//! Header geometry and WCS axis lookup.
use std::path::Path;

use fitsio::FitsFile;

use crate::error::{AtfitsError, Result};
use crate::keys::optional;

/// Shape and pixel metadata of a FITS primary image, in FITS axis order
/// (`dims[0]` = NAXIS1, the fastest-varying axis).
#[derive(Debug, Clone)]
pub struct HeaderGeom {
    pub naxis: usize,
    /// `dims[i]` = NAXIS(i+1).
    pub dims: Vec<usize>,
    pub bitpix: i64,
}

impl HeaderGeom {
    /// Read NAXIS, every NAXISn, and BITPIX from the primary HDU.
    pub fn read(path: &Path) -> Result<Self> {
        let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
        let hdu = fptr.primary_hdu()?;
        let naxis: i64 = hdu.read_key(&mut fptr, "NAXIS")?;
        let bitpix: i64 = hdu.read_key(&mut fptr, "BITPIX")?;
        let mut dims = Vec::with_capacity(naxis as usize);
        for i in 1..=naxis {
            let n: i64 = hdu.read_key(&mut fptr, &format!("NAXIS{i}"))?;
            dims.push(n as usize);
        }
        Ok(Self {
            naxis: naxis as usize,
            dims,
            bitpix,
        })
    }

    /// Whether the image is two-dimensional (a single plane).
    pub fn is_2d(&self) -> bool {
        self.naxis == 2
    }

    /// Number of pixels per plane = NAXIS1 × NAXIS2.
    pub fn plane_len(&self) -> usize {
        self.dims.first().copied().unwrap_or(1) * self.dims.get(1).copied().unwrap_or(1)
    }
}

/// Location of a target (FREQ/TIME) axis within a header's WCS.
#[derive(Debug, Clone)]
pub struct TargetAxis {
    /// FITS axis number (1-based), e.g. 3 for NAXIS3.
    pub fits_idx: usize,
    /// numpy/array index (0-based, axis order reversed), as used by `np.take`.
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

/// Search a header for the axis whose CTYPE contains `name` ("FREQ" or "TIME").
///
/// Missing CRPIX/CRVAL/CDELT take the FITS WCS defaults (0, 0, 1). Returns
/// [`AtfitsError::TargetAxisMissing`] when no axis matches.
pub fn find_target_axis(path: &Path, name: &str) -> Result<TargetAxis> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    let naxis: i64 = hdu.read_key(&mut fptr, "NAXIS")?;

    for axis in 1..=naxis {
        let Some(ctype) = optional(hdu.read_key::<String>(&mut fptr, &format!("CTYPE{axis}")))?
        else {
            continue;
        };
        if !ctype.contains(name) {
            continue;
        }
        let mut key = |k: String| optional(hdu.read_key::<f64>(&mut fptr, &k));
        let crpix = key(format!("CRPIX{axis}"))?.unwrap_or(0.0);
        let crval = key(format!("CRVAL{axis}"))?.unwrap_or(0.0);
        let cdelt = match key(format!("CD{axis}_{axis}"))? {
            Some(cd) => cd,
            None => key(format!("CDELT{axis}"))?.unwrap_or(1.0),
        };
        let cunit = optional(hdu.read_key::<String>(&mut fptr, &format!("CUNIT{axis}")))?;
        return Ok(TargetAxis {
            fits_idx: axis as usize,
            array_idx: (naxis - axis) as usize,
            ctype,
            crpix,
            crval,
            cdelt,
            cunit,
        });
    }
    Err(AtfitsError::TargetAxisMissing(format!(
        "No {name} axis found in WCS of {}",
        path.display()
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
