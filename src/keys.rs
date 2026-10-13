//! Header keyword editing and reading.
//!
//! `fitsio`'s `write_key` calls `ffpky*`, which appends a duplicate card when the
//! keyword exists. The `update_key_*` functions use `ffuky*`, which replaces it.
use std::ffi::CString;
use std::os::raw::c_int;
use std::path::Path;
use std::ptr;

use fitsio::FitsFile;
use fitsio::headers::ReadsKey;
use fitsio::sys;

use crate::error::{AtfitsError, Result, ffcall};

const KEY_NO_EXIST: i32 = sys::KEY_NO_EXIST as i32;

pub(crate) fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|e| AtfitsError::Other(format!("invalid C string {s:?}: {e}")))
}

fn is_missing_key(e: &AtfitsError) -> bool {
    matches!(e, AtfitsError::Fits(fitsio::errors::Error::Fits(f)) if f.status == KEY_NO_EXIST)
}

/// `Ok(None)` for a missing keyword, otherwise the read result.
pub(crate) fn optional<T>(r: fitsio::errors::Result<T>) -> Result<Option<T>> {
    match r.map_err(AtfitsError::from) {
        Ok(v) => Ok(Some(v)),
        Err(e) if is_missing_key(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Significant digits needed for `value` to survive a decimal round trip.
fn round_trip_digits(value: f64) -> c_int {
    (15..17)
        .find(|&d| format!("{value:.*e}", d as usize - 1).parse::<f64>() == Ok(value))
        .unwrap_or(17)
}

/// Set an integer keyword, replacing any existing card.
pub fn update_key_i64(fptr: &mut FitsFile, name: &str, value: i64) -> Result<()> {
    let name = cstr(name)?;
    ffcall(|s| unsafe { sys::ffukyj(fptr.as_raw(), name.as_ptr(), value, ptr::null_mut(), s) })
}

/// Set a float keyword, replacing any existing card.
///
/// Uses the fewest significant digits (15 to 17) that read back exactly.
pub fn update_key_f64(fptr: &mut FitsFile, name: &str, value: f64) -> Result<()> {
    let name = cstr(name)?;
    let decimals = -round_trip_digits(value);
    ffcall(|s| unsafe {
        sys::ffukyd(
            fptr.as_raw(),
            name.as_ptr(),
            value,
            decimals,
            ptr::null_mut(),
            s,
        )
    })
}

/// Set a string keyword, replacing any existing card.
pub fn update_key_str(fptr: &mut FitsFile, name: &str, value: &str) -> Result<()> {
    let name = cstr(name)?;
    let value = cstr(value)?;
    ffcall(|s| unsafe {
        sys::ffukys(
            fptr.as_raw(),
            name.as_ptr(),
            value.as_ptr(),
            ptr::null_mut(),
            s,
        )
    })
}

/// Set a logical (`T`/`F`) keyword, replacing any existing card.
///
/// casacore reads flags such as CASAMBM as logicals and rejects quoted strings.
pub fn update_key_logical(fptr: &mut FitsFile, name: &str, value: bool) -> Result<()> {
    let name = cstr(name)?;
    ffcall(|s| unsafe {
        sys::ffukyl(
            fptr.as_raw(),
            name.as_ptr(),
            value.into(),
            ptr::null_mut(),
            s,
        )
    })
}

/// Delete a keyword. A missing keyword is not an error.
pub fn delete_key(fptr: &mut FitsFile, name: &str) -> Result<()> {
    let name = cstr(name)?;
    match ffcall(|s| unsafe { sys::ffdkey(fptr.as_raw(), name.as_ptr(), s) }) {
        Err(e) if is_missing_key(&e) => Ok(()),
        r => r,
    }
}

/// Append a COMMENT card.
pub fn write_comment(fptr: &mut FitsFile, comment: &str) -> Result<()> {
    let comment = cstr(comment)?;
    ffcall(|s| unsafe { sys::ffpcom(fptr.as_raw(), comment.as_ptr(), s) })
}

/// Read a keyword of the current HDU as `i64`, or `None` if it is absent.
pub fn key_i64(fptr: &mut FitsFile, name: &str) -> Result<Option<i64>> {
    optional(i64::read_key(fptr, name))
}

/// Read a keyword of the current HDU as `f64`, or `None` if it is absent.
///
/// A value that is not a number is an error.
pub fn key_f64(fptr: &mut FitsFile, name: &str) -> Result<Option<f64>> {
    optional(f64::read_key(fptr, name))
}

/// Read a keyword of the current HDU as a string, or `None` if it is absent.
pub fn key_string(fptr: &mut FitsFile, name: &str) -> Result<Option<String>> {
    optional(String::read_key(fptr, name))
}

/// Whether the current HDU has the keyword, whatever its value type.
pub fn key_exists(fptr: &mut FitsFile, name: &str) -> Result<bool> {
    let name = cstr(name)?;
    let mut card = [0; 81];
    match ffcall(|s| unsafe { sys::ffgcrd(fptr.as_raw(), name.as_ptr(), card.as_mut_ptr(), s) }) {
        Ok(()) => Ok(true),
        Err(e) if is_missing_key(&e) => Ok(false),
        Err(e) => Err(e),
    }
}

fn open_primary(path: &Path) -> Result<FitsFile> {
    let mut fptr = FitsFile::open(path)?;
    fptr.primary_hdu()?;
    Ok(fptr)
}

/// [`key_f64`] on the primary header of the file at `path`.
pub fn read_key_f64(path: &Path, key: &str) -> Result<Option<f64>> {
    key_f64(&mut open_primary(path)?, key)
}

/// [`key_string`] on the primary header of the file at `path`.
pub fn read_key_string(path: &Path, key: &str) -> Result<Option<String>> {
    key_string(&mut open_primary(path)?, key)
}

/// [`key_exists`] on the primary header of the file at `path`.
pub fn has_key(path: &Path, key: &str) -> Result<bool> {
    key_exists(&mut open_primary(path)?, key)
}
