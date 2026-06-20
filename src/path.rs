//! Output-path construction helpers.
use std::path::{Path, PathBuf};

/// Build an output path from `input` with an optional suffix/prefix/outdir.
///
/// With `suffix = Some("smooth")` and input `img.fits`, yields `img.smooth.fits`;
/// `prefix` is prepended to the filename; `outdir` overrides the directory.
pub fn output_path(
    input: &Path,
    suffix: Option<&str>,
    prefix: Option<&str>,
    outdir: Option<&Path>,
) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let ext = input.extension().unwrap_or_default().to_string_lossy();

    let filename = match suffix {
        Some(s) => format!("{stem}.{s}.{ext}"),
        None => format!("{stem}.{ext}"),
    };
    let filename = match prefix {
        Some(p) => format!("{p}{filename}"),
        None => filename,
    };

    let dir = outdir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")));
    dir.join(filename)
}
