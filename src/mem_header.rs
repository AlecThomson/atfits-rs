//! Build a cube header in memory so the data unit can be written with plain
//! file I/O.
//!
//! cfitsio fills the data unit with zeros when an image is closed, which costs a
//! full extra write for a large cube. Instead, build the header with
//! [`create_mem_cube`], write [`CubeLayout::header`] to the file, extend the
//! file to its final length (sparse), and write the planes at
//! [`CubeLayout::datastart`] as big-endian bytes.
use std::os::raw::c_int;
use std::path::Path;
use std::ptr;

use fitsio::FitsFile;
use fitsio::sys::{self, LONGLONG, fitsfile};

use crate::error::{AtfitsError, Result, ffcall};
use crate::image::{copy_template_cards, serialise_header};
use crate::path::utf8;

/// A primary header ready to write at the start of a file.
pub struct CubeLayout {
    /// Header bytes, a whole number of 2880-byte blocks.
    pub header: Vec<u8>,
    /// Byte offset of the data unit, equal to `header.len()`.
    pub datastart: u64,
}

/// Create an in-memory image HDU of type `bitpix` with `dims.len()` axes and
/// copy the header cards of `template` that still apply.
///
/// The axes have length 1 so cfitsio never allocates the data unit in memory;
/// [`extract_header_layout`] writes the real lengths. Edit keywords on the
/// returned handle, then pass it and the same `dims` to
/// [`extract_header_layout`].
pub fn create_mem_cube(template: &Path, bitpix: i64, dims: &[usize]) -> Result<FitsFile> {
    let mut raw: *mut fitsfile = ptr::null_mut();
    ffcall(|s| unsafe { sys::ffinit(&mut raw, c"mem://".as_ptr(), s) })?;
    let mut out = unsafe { FitsFile::from_raw(raw, fitsio::FileOpenMode::READWRITE)? };

    let mut naxes: Vec<LONGLONG> = vec![1; dims.len()];
    ffcall(|s| unsafe {
        sys::ffcrimll(
            out.as_raw(),
            bitpix as c_int,
            naxes.len() as c_int,
            naxes.as_mut_ptr(),
            s,
        )
    })?;

    let mut src = FitsFile::open(utf8(template)?)?;
    src.primary_hdu()?;
    copy_template_cards(&mut src, &mut out, bitpix)?;
    Ok(out)
}

/// Serialise the primary header of a [`create_mem_cube`] handle with NAXISn set
/// from `dims` (`dims[0]` = NAXIS1).
pub fn extract_header_layout(fptr: &mut FitsFile, dims: &[usize]) -> Result<CubeLayout> {
    fptr.primary_hdu()?;
    let mut header = serialise_header(fptr, &[], 0)?;
    for (i, &d) in dims.iter().enumerate() {
        set_naxis_card(&mut header, i + 1, d)?;
    }
    let datastart = header.len() as u64;
    Ok(CubeLayout { header, datastart })
}

/// Replace the value of the `NAXIS{axis}` card in serialised header bytes.
fn set_naxis_card(header: &mut [u8], axis: usize, value: usize) -> Result<()> {
    let key = format!("NAXIS{axis}");
    let card = header
        .as_chunks_mut::<80>()
        .0
        .iter_mut()
        .find(|c| c.starts_with(key.as_bytes()) && c[key.len()] == b' ')
        .ok_or_else(|| AtfitsError::Other(format!("{key} card not found in header")))?;
    // Fixed-format integers are right-justified in bytes 10..30.
    let digits = value.to_string();
    let field = &mut card[10..30];
    field.fill(b' ');
    field[20 - digits.len()..].copy_from_slice(digits.as_bytes());
    Ok(())
}
