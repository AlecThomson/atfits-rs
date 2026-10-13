//! Image geometry and WCS axis lookup.
use std::path::Path;

use fitsio::FitsFile;

use crate::error::{AtfitsError, Result};
use crate::keys::{key_f64, key_i64, key_string};
use crate::path::utf8;

/// Shape and BITPIX of an image, with `dims[i]` = NAXIS(i+1).
#[derive(Debug, Clone)]
pub struct HeaderGeom {
    pub naxis: usize,
    pub dims: Vec<usize>,
    pub bitpix: i64,
}

impl HeaderGeom {
    /// Read the primary image geometry of the file at `path`.
    pub fn read(path: &Path) -> Result<Self> {
        let mut fptr = FitsFile::open(utf8(path)?)?;
        fptr.primary_hdu()?;
        Self::from_file(&mut fptr)
    }

    /// Read the geometry of the current HDU.
    pub fn from_file(fptr: &mut FitsFile) -> Result<Self> {
        let naxis = required_i64(fptr, "NAXIS")?;
        let bitpix = required_i64(fptr, "BITPIX")?;
        let dims = (1..=naxis)
            .map(|i| required_i64(fptr, &format!("NAXIS{i}")).map(|n| n as usize))
            .collect::<Result<_>>()?;
        Ok(Self {
            naxis: naxis as usize,
            dims,
            bitpix,
        })
    }

    /// Whether the image is a single plane.
    pub fn is_2d(&self) -> bool {
        self.naxis == 2
    }

    /// Pixels per plane, NAXIS1 * NAXIS2.
    pub fn plane_len(&self) -> usize {
        self.dims.iter().take(2).product()
    }
}

fn required_i64(fptr: &mut FitsFile, key: &str) -> Result<i64> {
    key_i64(fptr, key)?.ok_or_else(|| AtfitsError::MissingKeyword(key.to_string()))
}

/// A WCS axis found by [`find_target_axis`].
#[derive(Debug, Clone)]
pub struct TargetAxis {
    /// 1-based FITS axis number, e.g. 3 for NAXIS3.
    pub fits_idx: usize,
    /// 0-based index in C (numpy) axis order.
    pub array_idx: usize,
    pub ctype: String,
    /// CRPIXi, default 0.
    pub crpix: f64,
    /// CRVALi, default 0.
    pub crval: f64,
    /// CDi_i if present, else CDELTi, else 1. PCi_j is not applied.
    pub cdelt: f64,
    pub cunit: Option<String>,
}

impl TargetAxis {
    /// World coordinate of 1-based pixel `pix` along this axis, in CUNIT.
    pub fn world(&self, pix: f64) -> f64 {
        self.crval + (pix - self.crpix) * self.cdelt
    }
}

/// Find the primary header axis whose CTYPE contains `name` ("FREQ" or "TIME").
///
/// Missing CRPIX/CRVAL/CDELT take the FITS WCS defaults (0, 0, 1). Returns
/// [`AtfitsError::TargetAxisMissing`] when no axis matches.
pub fn find_target_axis(path: &Path, name: &str) -> Result<TargetAxis> {
    let mut fptr = FitsFile::open(utf8(path)?)?;
    fptr.primary_hdu()?;
    find_target_axis_in(&mut fptr, name)
}

/// [`find_target_axis`] on the current HDU of an open file.
pub fn find_target_axis_in(fptr: &mut FitsFile, name: &str) -> Result<TargetAxis> {
    let naxis = required_i64(fptr, "NAXIS")? as usize;
    for axis in 1..=naxis {
        let Some(ctype) = key_string(fptr, &format!("CTYPE{axis}"))? else {
            continue;
        };
        if !ctype.contains(name) {
            continue;
        }
        let cdelt = match key_f64(fptr, &format!("CD{axis}_{axis}"))? {
            Some(cd) => cd,
            None => key_f64(fptr, &format!("CDELT{axis}"))?.unwrap_or(1.0),
        };
        return Ok(TargetAxis {
            fits_idx: axis,
            array_idx: naxis - axis,
            crpix: key_f64(fptr, &format!("CRPIX{axis}"))?.unwrap_or(0.0),
            crval: key_f64(fptr, &format!("CRVAL{axis}"))?.unwrap_or(0.0),
            cdelt,
            cunit: key_string(fptr, &format!("CUNIT{axis}"))?,
            ctype,
        });
    }
    Err(AtfitsError::TargetAxisMissing(format!(
        "No {name} axis found in WCS of {}",
        fptr.file_path().display()
    )))
}

/// Factor converting a frequency or time unit to Hz or seconds.
///
/// `None` (no CUNIT) means the SI default and gives 1. Case is ignored, so
/// "MHZ" is megahertz. Returns `None` for units it does not know.
pub fn spectral_unit_scale(unit: Option<&str>) -> Option<f64> {
    let Some(unit) = unit else {
        return Some(1.0);
    };
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "hz" | "s" => 1.0,
        "khz" => 1e3,
        "mhz" => 1e6,
        "ghz" => 1e9,
        "thz" => 1e12,
        "ms" => 1e-3,
        "us" => 1e-6,
        "min" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => return None,
    };
    Some(scale)
}
