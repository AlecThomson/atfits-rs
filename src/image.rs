//! Image-HDU creation, resizing, and header-only copying.
use std::path::Path;

use fitsio::FitsFile;
use fitsio::errors::check_status;

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

/// Keywords that describe the on-disk array structure; these are set by
/// `fits_create_img` for a new image and must NOT be copied from a template.
pub fn is_structural_keyword(name: &str) -> bool {
    matches!(
        name,
        "SIMPLE"
            | "BITPIX"
            | "NAXIS"
            | "EXTEND"
            | "PCOUNT"
            | "GCOUNT"
            | "END"
            | "BSCALE"
            | "BZERO"
            | "BLANK"
    ) || (name.starts_with("NAXIS") && name[5..].chars().all(|c| c.is_ascii_digit()))
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

/// Refuse to clobber the input: this module deletes `output` before recreating
/// it, so if `output` resolves to `input` (an empty suffix, or a symlink) the
/// source image would be destroyed. `canonicalize` only succeeds for paths that
/// exist, so a brand-new output simply skips the check.
fn refuse_input_clobber(input: &Path, output: &Path) -> Result<()> {
    if let (Ok(in_canon), Ok(out_canon)) = (input.canonicalize(), output.canonicalize())
        && in_canon == out_canon
    {
        return Err(AtfitsError::Other(format!(
            "output {} resolves to the input image",
            output.display()
        )));
    }
    Ok(())
}

/// Create `output` from the primary-HDU header of `input` and return the
/// **open** FITS handle (no pixel data written yet).
///
/// Uses cfitsio `fits_create_file` (ffinit) + `fits_copy_header` (ffcphd): the
/// data unit is defined by the copied NAXIS keywords but is *not* written until
/// the handle is closed. We cannot use `FitsFile::create().open()` here — that
/// eagerly writes a default empty (NAXIS=0) primary HDU, and ffcphd then copies
/// *nothing* into it, leaving a zero-dimensional image (writing pixels later
/// fails with the misleading cfitsio error 302).
///
/// Keeping the handle open lets the caller write every plane before the single
/// close, so cfitsio writes the data unit exactly once. The returned [`FitsFile`]
/// owns the raw pointer; dropping it closes the file exactly once.
pub fn copy_header_only_open(input: &Path, output: &Path) -> Result<FitsFile> {
    refuse_input_clobber(input, output)?;

    let mut in_fptr = FitsFile::open(input.to_string_lossy().as_ref())?;
    in_fptr.primary_hdu()?; // position at the primary HDU to copy

    if output.exists() {
        std::fs::remove_file(output)?;
    }
    let out_name = cstr(output.to_string_lossy().as_ref())?;

    let mut status = 0;
    let mut raw_out: *mut fitsio::sys::fitsfile = std::ptr::null_mut();
    let handle = unsafe {
        fitsio::sys::ffinit(&mut raw_out, out_name.as_ptr(), &mut status);
        check_status(status)?;

        fitsio::sys::ffcphd(in_fptr.as_raw(), raw_out, &mut status);
        if let Err(e) = check_status(status) {
            let mut close_status = 0;
            fitsio::sys::ffclos(raw_out, &mut close_status);
            return Err(e.into());
        }

        FitsFile::from_raw(raw_out, fitsio::FileOpenMode::READWRITE)?
    };
    Ok(handle)
}

/// Create `output` containing only the primary-HDU header of `input` — no pixel
/// data — closing the file immediately (which zero-fills the whole data unit).
///
/// For the streaming write path prefer [`copy_header_only_open`] or
/// [`create_cube_open`], which keep the handle open so the data unit is written
/// a single time.
pub fn copy_header_only(input: &Path, output: &Path) -> Result<()> {
    // Drop closes the handle, flushing the (zero-filled) data unit to disk.
    copy_header_only_open(input, output).map(drop)
}

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
    if output.exists() {
        std::fs::remove_file(output)?;
    }

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

    // Copy every non-structural card from the template's primary header.
    let mut in_fptr = FitsFile::open(input.to_string_lossy().as_ref())?;
    in_fptr.primary_hdu()?;
    let mut status = 0;
    unsafe {
        let mut nkeys: std::os::raw::c_int = 0;
        let mut morekeys: std::os::raw::c_int = 0;
        fitsio::sys::ffghsp(in_fptr.as_raw(), &mut nkeys, &mut morekeys, &mut status);
        check_status(status)?;

        let mut card = [0i8; 81];
        for i in 1..=nkeys {
            card.fill(0);
            fitsio::sys::ffgrec(in_fptr.as_raw(), i, card.as_mut_ptr(), &mut status);
            if check_status(status).is_err() {
                break;
            }
            let card_str = std::ffi::CStr::from_ptr(card.as_ptr()).to_string_lossy();
            let name = card_str.split([' ', '=']).next().unwrap_or("").trim();
            if is_structural_keyword(name) {
                continue;
            }
            fitsio::sys::ffprec(out.as_raw(), card.as_ptr(), &mut status);
            check_status(status)?;
        }
    }
    Ok(out)
}
