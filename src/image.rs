//! Image creation, resizing and header copying.
use std::ffi::{CStr, CString};
use std::io::Write;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::path::Path;

use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};
use fitsio::sys::{self, LONGLONG};

use crate::error::{AtfitsError, Result, ffcall};
use crate::keys::cstr;
use crate::path::utf8;

/// Blank cards reserved before END by [`copy_header_only`], so later keyword
/// updates do not make cfitsio shift the whole data unit.
const RESERVED_CARDS: usize = 36;

const END_CARD: &[u8; 80] =
    b"END                                                                             ";

/// Change the BITPIX and shape (`dims[0]` = NAXIS1) of the current image in
/// place, keeping the other header cards.
pub fn resize_image(fptr: &mut FitsFile, bitpix: i64, dims: &[usize]) -> Result<()> {
    let mut naxes: Vec<c_long> = dims.iter().map(|&d| d as c_long).collect();
    ffcall(|s| unsafe {
        sys::ffrsim(
            fptr.as_raw(),
            bitpix as c_int,
            naxes.len() as c_int,
            naxes.as_mut_ptr(),
            s,
        )
    })
}

/// Keywords fixed by the array layout, which a new image writes itself:
/// SIMPLE, BITPIX, NAXIS, NAXISn, EXTEND, PCOUNT, GCOUNT and END.
pub fn is_structural_keyword(name: &str) -> bool {
    matches!(
        name,
        "SIMPLE" | "BITPIX" | "NAXIS" | "EXTEND" | "PCOUNT" | "GCOUNT" | "END"
    ) || name
        .strip_prefix("NAXIS")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// BSCALE, BZERO and BLANK, which only apply to integer images.
pub fn is_scaling_keyword(name: &str) -> bool {
    matches!(name, "BSCALE" | "BZERO" | "BLANK")
}

/// Map a FITS BITPIX to the `fitsio` image type; unknown values give `Float`.
pub fn bitpix_to_image_type(bitpix: i64) -> ImageType {
    match bitpix {
        8 => ImageType::UnsignedByte,
        16 => ImageType::Short,
        32 => ImageType::Long,
        64 => ImageType::LongLong,
        -64 => ImageType::Double,
        _ => ImageType::Float,
    }
}

/// Write `input`'s primary header to a new file at `output` and size the file
/// for the data unit without writing it, so the data reads as zeros and the
/// file stays sparse.
///
/// CHECKSUM and DATASUM are dropped. An existing `output` is replaced, unless
/// it is `input`.
pub fn copy_header_only(input: &Path, output: &Path) -> Result<()> {
    refuse_input_clobber(input, output)?;
    let mut src = FitsFile::open(utf8(input)?)?;
    src.primary_hdu()?;
    let data_len = data_unit_len(&mut src)?;
    let header = serialise_header(&mut src, &["CHECKSUM", "DATASUM"], RESERVED_CARDS)?;
    drop(src);

    remove_existing(output)?;
    let mut file = std::fs::File::create_new(output)?;
    file.write_all(&header)?;
    file.set_len(header.len() as u64 + data_len)?;
    Ok(())
}

/// [`copy_header_only`], then open `output` for editing.
pub fn copy_header_only_open(input: &Path, output: &Path) -> Result<FitsFile> {
    copy_header_only(input, output)?;
    Ok(FitsFile::edit(utf8(output)?)?)
}

/// Create `output` as an image of shape `dims` (`dims[0]` = NAXIS1) and
/// `bitpix`, copy the header cards of `input` that still apply, and return the
/// open handle.
///
/// cfitsio writes the data unit when the handle closes, so writing every plane
/// before then writes the data once.
pub fn create_cube_open(
    input: &Path,
    output: &Path,
    bitpix: i64,
    dims: &[usize],
) -> Result<FitsFile> {
    refuse_input_clobber(input, output)?;
    remove_existing(output)?;

    let c_order: Vec<usize> = dims.iter().rev().copied().collect();
    let desc = ImageDescription {
        data_type: bitpix_to_image_type(bitpix),
        dimensions: &c_order,
    };
    let mut out = FitsFile::create(utf8(output)?)
        .with_custom_primary(&desc)
        .open()?;
    out.primary_hdu()?;

    let mut src = FitsFile::open(utf8(input)?)?;
    src.primary_hdu()?;
    copy_template_cards(&mut src, &mut out, bitpix)?;
    // Re-read the header so cfitsio applies the copied BSCALE/BZERO/BLANK.
    ffcall(|s| unsafe { sys::ffrdef(out.as_raw(), s) })?;
    Ok(out)
}

/// Whether a template card belongs in a new image of type `bitpix`.
fn keep_template_card(name: &str, bitpix: i64) -> bool {
    !(is_structural_keyword(name)
        || matches!(name, "CHECKSUM" | "DATASUM")
        || (bitpix < 0 && is_scaling_keyword(name)))
}

/// Append the cards of `src`'s current header that suit a `bitpix` image to
/// `dst`'s current header.
pub(crate) fn copy_template_cards(
    src: &mut FitsFile,
    dst: &mut FitsFile,
    bitpix: i64,
) -> Result<()> {
    let mut nkeys: c_int = 0;
    ffcall(|s| unsafe { sys::ffghsp(src.as_raw(), &mut nkeys, std::ptr::null_mut(), s) })?;

    let mut card = [0 as c_char; 81];
    for i in 1..=nkeys {
        ffcall(|s| unsafe { sys::ffgrec(src.as_raw(), i, card.as_mut_ptr(), s) })?;
        let name = card_name(unsafe { CStr::from_ptr(card.as_ptr()) }.to_bytes());
        if keep_template_card(name, bitpix) {
            ffcall(|s| unsafe { sys::ffprec(dst.as_raw(), card.as_ptr(), s) })?;
        }
    }
    Ok(())
}

/// Keyword name of a card: the text before the first space or `=`.
fn card_name(card: &[u8]) -> &str {
    let end = card
        .iter()
        .position(|&b| b == b' ' || b == b'=')
        .unwrap_or(card.len());
    std::str::from_utf8(&card[..end]).unwrap_or("")
}

/// Padded size in bytes of the current HDU's data unit.
fn data_unit_len(fptr: &mut FitsFile) -> Result<u64> {
    let (mut head, mut data, mut end): (LONGLONG, LONGLONG, LONGLONG) = (0, 0, 0);
    ffcall(|s| unsafe { sys::ffghadll(fptr.as_raw(), &mut head, &mut data, &mut end, s) })?;
    Ok((end - data) as u64)
}

/// The current header as FITS bytes: every card except `exclude`, `reserve`
/// blank cards, END, then blanks up to a whole 2880-byte block.
pub(crate) fn serialise_header(
    fptr: &mut FitsFile,
    exclude: &[&str],
    reserve: usize,
) -> Result<Vec<u8>> {
    let excl: Vec<CString> = exclude.iter().map(|k| cstr(k)).collect::<Result<_>>()?;
    let mut excl_ptrs: Vec<*mut c_char> = excl.iter().map(|c| c.as_ptr() as *mut c_char).collect();
    let mut text: *mut c_char = std::ptr::null_mut();
    let mut nkeys: c_int = 0;
    ffcall(|s| unsafe {
        sys::ffhdr2str(
            fptr.as_raw(),
            0,
            excl_ptrs.as_mut_ptr(),
            excl_ptrs.len() as c_int,
            &mut text,
            &mut nkeys,
            s,
        )
    })?;
    let mut header = unsafe { CStr::from_ptr(text) }.to_bytes().to_vec();
    ffcall(|s| unsafe { sys::fffree(text as *mut c_void, s) })?;

    // ffhdr2str ends with the END card; reserved space goes before it.
    if !header.ends_with(END_CARD) {
        return Err(AtfitsError::Other(
            "serialised header has no END card".into(),
        ));
    }
    header.truncate(header.len() - END_CARD.len());
    header.resize(header.len() + reserve * 80, b' ');
    header.extend_from_slice(END_CARD);
    header.resize(header.len().div_ceil(2880) * 2880, b' ');
    Ok(header)
}

/// Error if `output` is the same file as `input`, which the caller would delete.
fn refuse_input_clobber(input: &Path, output: &Path) -> Result<()> {
    if same_file(input, output) {
        return Err(AtfitsError::Other(format!(
            "output {} is the input image",
            output.display()
        )));
    }
    Ok(())
}

/// Inodes rather than paths, so hard links and case-insensitive names match.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Remove `path` if it exists, including a dangling symlink.
fn remove_existing(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
