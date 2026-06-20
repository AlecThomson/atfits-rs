//! Raw cfitsio header keyword editing and reading.
//!
//! `fitsio`'s `write_key` calls `ffpky*`, which *appends* a card even when the
//! keyword already exists, leaving duplicates that cfitsio then reads stale. The
//! `update_key_*` helpers here use `ffuky*` (`fits_update_key`) so a rewritten
//! keyword (CRVAL/CDELT/CTYPE/BMAJ/…) overwrites in place.
use std::ffi::CString;
use std::path::Path;

use fitsio::FitsFile;
use fitsio::errors::check_status;

use crate::error::{AtfitsError, Result};

pub(crate) fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|e| AtfitsError::Other(format!("invalid C string {s:?}: {e}")))
}

/// `fits_update_key_lng` — update/insert an integer keyword in place.
pub fn update_key_i64(fptr: &mut FitsFile, name: &str, value: i64) -> Result<()> {
    let c_name = cstr(name)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffukyj(
            fptr.as_raw(),
            c_name.as_ptr(),
            value,
            std::ptr::null_mut(),
            &mut status,
        );
    }
    check_status(status)?;
    Ok(())
}

/// `fits_update_key_dbl` — update/insert a double keyword in place.
///
/// `decimals = -15` asks cfitsio for the shortest decimal that round-trips.
pub fn update_key_f64(fptr: &mut FitsFile, name: &str, value: f64) -> Result<()> {
    let c_name = cstr(name)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffukyd(
            fptr.as_raw(),
            c_name.as_ptr(),
            value,
            -15,
            std::ptr::null_mut(),
            &mut status,
        );
    }
    check_status(status)?;
    Ok(())
}

/// `fits_update_key_str` — update/insert a string keyword in place.
pub fn update_key_str(fptr: &mut FitsFile, name: &str, value: &str) -> Result<()> {
    let c_name = cstr(name)?;
    let c_val = cstr(value)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffukys(
            fptr.as_raw(),
            c_name.as_ptr(),
            c_val.as_ptr(),
            std::ptr::null_mut(),
            &mut status,
        );
    }
    check_status(status)?;
    Ok(())
}

/// `fits_update_key_log` — update/insert a logical (boolean) keyword in place.
///
/// FITS logicals are unquoted `T`/`F`; casacore/CARTA read keywords like
/// `CASAMBM` with `asBool`, which throws on a quoted string — always write a
/// true logical.
pub fn update_key_logical(fptr: &mut FitsFile, name: &str, value: bool) -> Result<()> {
    let c_name = cstr(name)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffukyl(
            fptr.as_raw(),
            c_name.as_ptr(),
            value as std::os::raw::c_int,
            std::ptr::null_mut(),
            &mut status,
        );
    }
    check_status(status)?;
    Ok(())
}

/// `fits_delete_key` — remove a keyword if present (absence is not an error).
pub fn delete_key(fptr: &mut FitsFile, name: &str) -> Result<()> {
    let c_name = cstr(name)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffdkey(fptr.as_raw(), c_name.as_ptr(), &mut status);
    }
    // KEY_NO_EXIST (202) is fine — nothing to delete.
    if status == 202 {
        return Ok(());
    }
    check_status(status)?;
    Ok(())
}

/// `fits_write_comment` — append a COMMENT card.
pub fn write_comment(fptr: &mut FitsFile, comment: &str) -> Result<()> {
    let c_comment = cstr(comment)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffpcom(fptr.as_raw(), c_comment.as_ptr(), &mut status);
    }
    check_status(status)?;
    Ok(())
}

// ── Convenience readers (open by path) ────────────────────────────────────────

/// Read a header keyword as `f64`, or `None` if it is absent.
pub fn read_key_f64(path: &Path, key: &str) -> Result<Option<f64>> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    Ok(hdu.read_key::<f64>(&mut fptr, key).ok())
}

/// Read a header keyword as `String`, or `None` if it is absent.
pub fn read_key_string(path: &Path, key: &str) -> Result<Option<String>> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    Ok(hdu.read_key::<String>(&mut fptr, key).ok())
}

/// True if the keyword is present in the primary header.
pub fn has_key(path: &Path, key: &str) -> Result<bool> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    Ok(hdu.read_key::<String>(&mut fptr, key).is_ok()
        || hdu.read_key::<f64>(&mut fptr, key).is_ok())
}
