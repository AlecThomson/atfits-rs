//! Image geometry and WCS axis lookup.
use std::path::Path;

use fitsio::FitsFile;

use crate::error::{AtfitsError, Result};
use crate::keys::{key_exists, key_f64, key_i64, key_string};
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
    /// Pixel step: CDi_i if present, else CDELTi * PCi_i (defaults 1).
    pub cdelt: f64,
    pub cunit: Option<String>,
}

impl TargetAxis {
    /// World coordinate of 1-based pixel `pix` along this axis, in CUNIT.
    pub fn world(&self, pix: f64) -> f64 {
        self.crval + (pix - self.crpix) * self.cdelt
    }

    /// Factor converting CUNIT to Hz for a FREQ axis or to seconds for a TIME
    /// axis. `None` for an unknown unit or another axis type.
    pub fn unit_scale(&self) -> Option<f64> {
        let unit = self.cunit.as_deref();
        if self.ctype.contains("FREQ") {
            frequency_unit_scale(unit)
        } else if self.ctype.contains("TIME") {
            time_unit_scale(unit)
        } else {
            None
        }
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
            None => {
                key_f64(fptr, &format!("CDELT{axis}"))?.unwrap_or(1.0)
                    * key_f64(fptr, &format!("PC{axis}_{axis}"))?.unwrap_or(1.0)
            }
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
/// "MHZ" is megahertz. Returns `None` for units it does not know. Prefer
/// [`frequency_unit_scale`], [`time_unit_scale`] or [`TargetAxis::unit_scale`],
/// which reject units of the wrong kind.
pub fn spectral_unit_scale(unit: Option<&str>) -> Option<f64> {
    frequency_unit_scale(unit).or_else(|| time_unit_scale(unit))
}

/// Factor converting a frequency unit to Hz; no unit gives 1. Case is
/// ignored, so "MHZ" is megahertz. `None` for anything else.
pub fn frequency_unit_scale(unit: Option<&str>) -> Option<f64> {
    unit_scale(
        unit,
        &[
            ("hz", 1.0),
            ("khz", 1e3),
            ("mhz", 1e6),
            ("ghz", 1e9),
            ("thz", 1e12),
        ],
    )
}

/// Factor converting a time unit to seconds; no unit gives 1. `None` for
/// anything else.
pub fn time_unit_scale(unit: Option<&str>) -> Option<f64> {
    unit_scale(
        unit,
        &[
            ("s", 1.0),
            ("ms", 1e-3),
            ("us", 1e-6),
            ("min", 60.0),
            ("h", 3600.0),
            ("d", 86400.0),
        ],
    )
}

fn unit_scale(unit: Option<&str>, table: &[(&str, f64)]) -> Option<f64> {
    let unit = unit.unwrap_or("").trim().to_ascii_lowercase();
    if unit.is_empty() {
        return Some(1.0);
    }
    table.iter().find(|(u, _)| *u == unit).map(|&(_, f)| f)
}

/// `(|dx|, |dy|)`: the size in degrees of one pixel step along NAXIS1 and
/// NAXIS2 of the current HDU.
///
/// Uses the CD matrix when present, else CDELTi with PCi_j. CUNIT1/2 may be
/// deg (the default), arcmin, arcsec or rad.
pub fn pixel_scale_deg(fptr: &mut FitsFile) -> Result<(f64, f64)> {
    let mut has_cd = false;
    for key in ["CD1_1", "CD1_2", "CD2_1", "CD2_2"] {
        has_cd |= key_exists(fptr, key)?;
    }
    // m[i][j]: degrees along world axis i per pixel step along axis j.
    let mut m = [[0.0; 2]; 2];
    for i in 1..=2 {
        let row_scale = if has_cd {
            1.0
        } else {
            key_f64(fptr, &format!("CDELT{i}"))?
                .ok_or_else(|| AtfitsError::MissingKeyword(format!("CDELT{i} or CD{i}_{i}")))?
        };
        let unit = key_string(fptr, &format!("CUNIT{i}"))?;
        let deg = angle_unit_deg(unit.as_deref())
            .ok_or_else(|| AtfitsError::Other(format!("unknown angle unit CUNIT{i}={unit:?}")))?;
        for j in 1..=2 {
            let v = if has_cd {
                key_f64(fptr, &format!("CD{i}_{j}"))?.unwrap_or(0.0)
            } else {
                let diag = if i == j { 1.0 } else { 0.0 };
                key_f64(fptr, &format!("PC{i}_{j}"))?.unwrap_or(diag)
            };
            m[i - 1][j - 1] = v * row_scale * deg;
        }
    }
    Ok((m[0][0].hypot(m[1][0]), m[0][1].hypot(m[1][1])))
}

fn angle_unit_deg(unit: Option<&str>) -> Option<f64> {
    unit_scale(
        unit,
        &[
            ("deg", 1.0),
            ("arcmin", 1.0 / 60.0),
            ("arcsec", 1.0 / 3600.0),
            ("rad", 180.0 / std::f64::consts::PI),
        ],
    )
}
