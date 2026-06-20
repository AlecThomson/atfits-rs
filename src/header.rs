//! Header geometry and WCS axis lookup.
use std::path::Path;

use fitsio::FitsFile;

use crate::error::{AtfitsError, Result};

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
    pub crpix: f64,
    pub crval: f64,
    pub cdelt: f64,
    pub cunit: Option<String>,
}

/// Search a header for the axis whose CTYPE contains `name` ("FREQ" or "TIME").
///
/// Returns [`AtfitsError::TargetAxisMissing`] when absent.
pub fn find_target_axis(path: &Path, name: &str) -> Result<TargetAxis> {
    let mut fptr = FitsFile::open(path.to_string_lossy().as_ref())?;
    let hdu = fptr.primary_hdu()?;
    let naxis: i64 = hdu.read_key(&mut fptr, "NAXIS")?;

    for axis in 1..=naxis {
        let ctype: String = match hdu.read_key(&mut fptr, &format!("CTYPE{axis}")) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if ctype.contains(name) {
            let crpix: f64 = hdu
                .read_key(&mut fptr, &format!("CRPIX{axis}"))
                .unwrap_or(1.0);
            let crval: f64 = hdu
                .read_key(&mut fptr, &format!("CRVAL{axis}"))
                .unwrap_or(0.0);
            let cdelt: f64 = hdu
                .read_key(&mut fptr, &format!("CDELT{axis}"))
                .unwrap_or(1.0);
            let cunit: Option<String> = hdu.read_key(&mut fptr, &format!("CUNIT{axis}")).ok();
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
    }
    Err(AtfitsError::TargetAxisMissing(format!(
        "No {name} axis found in WCS of {}",
        path.display()
    )))
}
