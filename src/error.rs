//! Error type for the helpers.
use std::ffi::CStr;
use std::os::raw::c_char;

use thiserror::Error;

/// Errors raised by the helpers.
#[derive(Debug, Error)]
pub enum AtfitsError {
    /// cfitsio or `fitsio` error.
    #[error("FITS I/O error: {0}")]
    Fits(#[from] fitsio::errors::Error),

    /// Filesystem error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A required header keyword was absent.
    #[error("missing header keyword: {0}")]
    MissingKeyword(String),

    /// No WCS axis matched the requested CTYPE (e.g. FREQ or TIME).
    #[error("target axis missing: {0}")]
    TargetAxisMissing(String),

    /// A FITS image had an unexpected dimensionality.
    #[error("unsupported NAXIS={0}")]
    UnsupportedNaxis(i64),

    /// Any other failure, such as a bad argument or malformed header.
    #[error("{0}")]
    Other(String),
}

/// Result alias for the helpers.
pub type Result<T> = std::result::Result<T, AtfitsError>;

/// cfitsio's description of a status code, e.g. 202 gives "keyword not found in header".
pub fn status_message(status: i32) -> String {
    let mut buf = [0 as c_char; 31];
    unsafe {
        fitsio::sys::ffgerr(status, buf.as_mut_ptr());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

/// Whether the linked cfitsio was built thread safe (`--enable-reentrant`).
///
/// The `fitsio-src` build always is. A system library may not be.
pub fn is_reentrant() -> bool {
    unsafe { fitsio::sys::fits_is_reentrant() != 0 }
}

/// Run a cfitsio call that reports through a status pointer.
pub(crate) fn ffcall(f: impl FnOnce(&mut i32) -> i32) -> Result<()> {
    let mut status = 0;
    f(&mut status);
    fitsio::errors::check_status(status)?;
    Ok(())
}
