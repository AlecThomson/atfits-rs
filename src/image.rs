//! Image-HDU creation, resizing, and header-only copying.
use std::ffi::{CStr, CString};
use std::io::Write;
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;

use fitsio::FitsFile;
use fitsio::errors::check_status;
use fitsio::sys::LONGLONG;

use crate::error::{AtfitsError, Result};
use crate::keys::cstr;

/// `fits_resize_img` — change the BITPIX and shape of the primary image in
/// place, preserving all other header cards.
///
/// `dims` is in FITS order (`dims[0]` = NAXIS1). cfitsio zero-fills any new
/// pixels (sparsely) when the file is closed.
pub fn resize_image(fptr: &mut FitsFile, bitpix: i64, dims: &[usize]) -> Result<()> {
    let mut naxes: Vec<std::os::raw::c_long> =
        dims.iter().map(|&d| d as std::os::raw::c_long).collect();
    let mut status = 0;
    unsafe {
        fitsio::sys::ffrsim(
            fptr.as_raw(),
            bitpix as std::os::raw::c_int,
            naxes.len() as std::os::raw::c_int,
            naxes.as_mut_ptr(),
            &mut status,
        );
    }
    check_status(status)?;
    Ok(())
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

/// BSCALE, BZERO and BLANK: meaningful only for integer images.
pub fn is_scaling_keyword(name: &str) -> bool {
    matches!(name, "BSCALE" | "BZERO" | "BLANK")
}

/// Whether a template card belongs in a new image of type `bitpix`.
///
/// Checksums are dropped because they describe the template's data, not ours.
fn keep_template_card(name: &str, bitpix: i64) -> bool {
    !(is_structural_keyword(name)
        || matches!(name, "CHECKSUM" | "DATASUM")
        || (bitpix < 0 && is_scaling_keyword(name)))
}

/// Append every card of `src`'s current header that [`keep_template_card`]
/// accepts to `dst`'s current header.
pub(crate) fn copy_template_cards(
    src: &mut FitsFile,
    dst: &mut FitsFile,
    bitpix: i64,
) -> Result<()> {
    let mut status = 0;
    let mut nkeys: c_int = 0;
    let mut morekeys: c_int = 0;
    unsafe { fitsio::sys::ffghsp(src.as_raw(), &mut nkeys, &mut morekeys, &mut status) };
    check_status(status)?;

    let mut card = [0 as c_char; 81];
    for i in 1..=nkeys {
        unsafe { fitsio::sys::ffgrec(src.as_raw(), i, card.as_mut_ptr(), &mut status) };
        check_status(status)?;
        let name = card_name(unsafe { CStr::from_ptr(card.as_ptr()) }.to_bytes());
        if keep_template_card(name, bitpix) {
            unsafe { fitsio::sys::ffprec(dst.as_raw(), card.as_ptr(), &mut status) };
            check_status(status)?;
        }
    }
    Ok(())
}

/// Keyword name of a raw 80-byte card (columns 1-8, or up to `=` for HIERARCH).
fn card_name(card: &[u8]) -> &str {
    let end = card
        .iter()
        .position(|&b| b == b' ' || b == b'=')
        .unwrap_or(card.len());
    std::str::from_utf8(&card[..end]).unwrap_or("")
}

/// Map a FITS `BITPIX` to the `fitsio` image element type.
pub fn bitpix_to_image_type(bitpix: i64) -> fitsio::images::ImageType {
    use fitsio::images::ImageType;
    match bitpix {
        8 => ImageType::UnsignedByte,
        16 => ImageType::Short,
        32 => ImageType::Long,
        64 => ImageType::LongLong,
        -64 => ImageType::Double,
        _ => ImageType::Float, // -32 and anything unexpected
    }
}

/// Error if `output` is the same file as `input`, since callers delete `output`
/// before writing it.
fn refuse_input_clobber(input: &Path, output: &Path) -> Result<()> {
    if same_file(input, output) {
        return Err(AtfitsError::Other(format!(
            "output {} is the input image",
            output.display()
        )));
    }
    Ok(())
}

/// Compares inodes on unix so case-insensitive filesystems and hard links count.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }
}

/// Remove `path` if it exists, including a dangling symlink.
fn remove_existing(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Blank cards reserved before END so callers can add keywords without
/// cfitsio shifting the whole data unit.
const RESERVED_CARDS: usize = 36;

/// Write `input`'s primary header to a new `output` and size the file for the
/// data unit without writing it, so the data reads as zeros and stays sparse.
///
/// CHECKSUM and DATASUM are dropped since the data no longer matches them.
pub fn copy_header_only(input: &Path, output: &Path) -> Result<()> {
    refuse_input_clobber(input, output)?;
    let mut in_fptr = FitsFile::open(input.to_string_lossy().as_ref())?;
    in_fptr.primary_hdu()?;
    let (_, datastart, dataend) = hdu_offsets(&mut in_fptr)?;
    let header = serialise_header(&mut in_fptr, &["CHECKSUM", "DATASUM"], RESERVED_CARDS)?;
    drop(in_fptr);

    remove_existing(output)?;
    let mut file = std::fs::File::create_new(output)?;
    file.write_all(&header)?;
    file.set_len(header.len() as u64 + (dataend - datastart))?;
    Ok(())
}

/// [`copy_header_only`], then open `output` for writing.
pub fn copy_header_only_open(input: &Path, output: &Path) -> Result<FitsFile> {
    copy_header_only(input, output)?;
    Ok(FitsFile::edit(output.to_string_lossy().as_ref())?)
}

/// Header start, data start and data end (block padded) of the current HDU.
pub(crate) fn hdu_offsets(fptr: &mut FitsFile) -> Result<(u64, u64, u64)> {
    let (mut head, mut data, mut end): (LONGLONG, LONGLONG, LONGLONG) = (0, 0, 0);
    let mut status = 0;
    unsafe { fitsio::sys::ffghadll(fptr.as_raw(), &mut head, &mut data, &mut end, &mut status) };
    check_status(status)?;
    Ok((head as u64, data as u64, end as u64))
}

/// The current header as FITS bytes: every card except `exclude`, `reserve`
/// blank cards, END, then space padding to a whole 2880-byte block.
pub(crate) fn serialise_header(
    fptr: &mut FitsFile,
    exclude: &[&str],
    reserve: usize,
) -> Result<Vec<u8>> {
    let excl: Vec<CString> = exclude.iter().map(|k| cstr(k)).collect::<Result<_>>()?;
    let mut excl_ptrs: Vec<*mut c_char> = excl.iter().map(|c| c.as_ptr() as *mut c_char).collect();
    let mut text: *mut c_char = std::ptr::null_mut();
    let mut nkeys: c_int = 0;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffhdr2str(
            fptr.as_raw(),
            0,
            excl_ptrs.as_mut_ptr(),
            excl_ptrs.len() as c_int,
            &mut text,
            &mut nkeys,
            &mut status,
        );
    }
    check_status(status)?;
    let mut header = unsafe {
        let bytes = CStr::from_ptr(text).to_bytes().to_vec();
        let mut free_status = 0;
        fitsio::sys::fffree(text as *mut c_void, &mut free_status);
        bytes
    };
    // ffhdr2str ends the string with the END card.
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

const END_CARD: &[u8; 80] =
    b"END                                                                             ";

/// Create `output` as a fresh image of shape `dims` (FITS order, NAXIS1 first)
/// and `bitpix`, copy every non-structural header card from `input`, and return
/// the **open** handle.
///
/// Keeping the handle open is the key to performance: cfitsio writes the (large)
/// data unit to disk only when the file is closed, so if the caller streams all
/// planes into this handle before closing, the data is written exactly once.
/// `copy_header_only` + [`resize_image`] instead closes after resizing, forcing
/// cfitsio to write a full pass of zeros that every plane then overwrites —
/// doubling the I/O for big cubes.
pub fn create_cube_open(
    input: &Path,
    output: &Path,
    bitpix: i64,
    dims: &[usize],
) -> Result<FitsFile> {
    use fitsio::images::ImageDescription;

    refuse_input_clobber(input, output)?;
    remove_existing(output)?;

    // `ImageDescription::dimensions` is C-order (row-major), the reverse of the
    // FITS NAXIS order.
    let c_dims: Vec<usize> = dims.iter().rev().copied().collect();
    let desc = ImageDescription {
        data_type: bitpix_to_image_type(bitpix),
        dimensions: &c_dims,
    };
    let mut out = FitsFile::create(output)
        .with_custom_primary(&desc)
        .overwrite()
        .open()?;
    out.primary_hdu()?;

    let mut in_fptr = FitsFile::open(input.to_string_lossy().as_ref())?;
    in_fptr.primary_hdu()?;
    copy_template_cards(&mut in_fptr, &mut out, bitpix)?;
    // Re-read the header so cfitsio applies any copied BSCALE/BZERO/BLANK.
    let mut status = 0;
    unsafe { fitsio::sys::ffrdef(out.as_raw(), &mut status) };
    check_status(status)?;
    Ok(out)
}
