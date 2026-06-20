//! Error type shared by the low-level cfitsio helpers.
//!
//! Consuming crates wrap this in their own error enum with
//! `#[error(...)] Atfits(#[from] atfits_rs::AtfitsError)` (or map individual
//! variants), so a `?` on any helper here flows into their `Result`.
use thiserror::Error;

/// Errors raised by the shared cfitsio helpers.
#[derive(Debug, Error)]
pub enum AtfitsError {
    /// Underlying cfitsio error.
    #[error("FITS I/O error: {0}")]
    Fits(#[from] fitsio::errors::Error),

    /// Filesystem I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A required header keyword was absent.
    #[error("missing header keyword: {0}")]
    MissingKeyword(String),

    /// A requested target (e.g. FREQ/TIME) axis was not found in the WCS.
    #[error("target axis missing: {0}")]
    TargetAxisMissing(String),

    /// A FITS image had an unexpected dimensionality.
    #[error("unsupported NAXIS={0}")]
    UnsupportedNaxis(i64),

    /// Catch-all for invariant violations.
    #[error("{0}")]
    Other(String),
}

/// Convenience result alias for the shared helpers.
pub type Result<T> = std::result::Result<T, AtfitsError>;
