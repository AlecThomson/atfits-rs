//! Raw cfitsio header keyword editing and reading.
//!
//! `fitsio`'s `write_key` calls `ffpky*`, which *appends* a card even when the
//! keyword already exists, leaving duplicates that cfitsio then reads stale. The
//! `update_key_*` helpers here use `ffuky*` (`fits_update_key`) so a rewritten
//! keyword (CRVAL/CDELT/CTYPE/BMAJ/…) overwrites in place.
use std::ffi::CString;
use std::os::raw::c_int;
use std::path::Path;

use fitsio::FitsFile;
use fitsio::errors::check_status;

use crate::error::{AtfitsError, Result};

const KEY_NO_EXIST: i32 = fitsio::sys::KEY_NO_EXIST as i32;

/// Significant digits needed for `value` to survive a decimal round trip.
fn round_trip_digits(value: f64) -> usize {
    (15..17)
        .find(|&d| format!("{value:.*e}", d - 1).parse::<f64>() == Ok(value))
        .unwrap_or(17)
}

/// `Ok(None)` for a missing keyword, otherwise the read result.
pub(crate) fn optional<T>(r: fitsio::errors::Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(fitsio::errors::Error::Fits(e)) if e.status == KEY_NO_EXIST => Ok(None),
        Err(e) => Err(e.into()),
    }
}

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
/// Written with the fewest significant digits (15 to 17) that read back exactly.
pub fn update_key_f64(fptr: &mut FitsFile, name: &str, value: f64) -> Result<()> {
    let c_name = cstr(name)?;
    let mut status = 0;
    unsafe {
        fitsio::sys::ffukyd(
            fptr.as_raw(),
            c_name.as_ptr(),
            value,
            -(round_trip_digits(value) as c_int),
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
    if status == KEY_NO_EXIST {
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

/// Read a primary header keyword as `f64`, or `None` if it is absent.
///
/// A value that is not a number is an error.
pub fn read_key_f64(path: &Path, key: &str) -> Result<Option<f64>> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    optional(hdu.read_key::<f64>(&mut fptr, key))
}

/// Read a primary header keyword as `String`, or `None` if it is absent.
pub fn read_key_string(path: &Path, key: &str) -> Result<Option<String>> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    optional(hdu.read_key::<String>(&mut fptr, key))
}

/// True if the keyword is present in the primary header.
pub fn has_key(path: &Path, key: &str) -> Result<bool> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    Ok(hdu.read_key::<String>(&mut fptr, key).is_ok()
        || hdu.read_key::<f64>(&mut fptr, key).is_ok())
}
