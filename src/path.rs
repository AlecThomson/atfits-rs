//! Output path construction.
use std::path::{Path, PathBuf};

use crate::error::{AtfitsError, Result};

/// Build an output path from `input`.
///
/// `img.fits` with suffix `smooth` gives `img.smooth.fits`; `prefix` is
/// prepended to the file name and `outdir` replaces the directory.
pub fn output_path(
    input: &Path,
    suffix: Option<&str>,
    prefix: Option<&str>,
    outdir: Option<&Path>,
) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let mut name = format!("{}{stem}", prefix.unwrap_or_default());
    if let Some(s) = suffix {
        name = format!("{name}.{s}");
    }
    if let Some(ext) = input.extension() {
        name = format!("{name}.{}", ext.to_string_lossy());
    }
    let dir = outdir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")));
    dir.join(name)
}

/// `path` as UTF-8, which `fitsio` requires (it panics otherwise).
pub(crate) fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| AtfitsError::Other(format!("path is not UTF-8: {}", path.display())))
}
