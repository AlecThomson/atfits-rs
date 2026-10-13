//! Pixel precision and monomorphic cfitsio section I/O.
use std::os::raw::{c_int, c_void};

use fitsio::FitsFile;
use fitsio::errors::check_status;
use fitsio::hdu::HduInfo;
use fitsio::images::ImageType;
use fitsio::sys::LONGLONG;

use crate::error::{AtfitsError, Result};

/// Pixel precision a cube is read/written in, derived from FITS `BITPIX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelType {
    F32,
    F64,
}

impl PixelType {
    /// `-64` maps to f64; `-32` and every integer BITPIX map to f32.
    pub fn from_bitpix(bitpix: i64) -> Self {
        if bitpix == -64 {
            PixelType::F64
        } else {
            PixelType::F32
        }
    }
}

/// Pixel element types a plane can be streamed in: `f32` or `f64`.
///
/// Sections are flat pixel ranges `start..end` of the primary image. Integer
/// images are scaled by BSCALE/BZERO; pixels equal to BLANK read as NaN, and NaN
/// is written back as BLANK (an error if the header has no BLANK).
pub trait CubeElem: Copy + Send + Sync + 'static {
    /// FITS `BITPIX` written for this element type.
    const BITPIX: i64;
    fn read_full(fptr: &mut FitsFile) -> Result<Vec<Self>>;
    fn read_section(fptr: &mut FitsFile, start: usize, end: usize) -> Result<Vec<Self>>;
    fn write_section(fptr: &mut FitsFile, start: usize, end: usize, data: &[Self]) -> Result<()>;
}

/// Primary image pixel count and whether it stores integers.
fn primary_image(fptr: &mut FitsFile) -> Result<(usize, bool)> {
    let hdu = fptr.primary_hdu()?;
    match hdu.info {
        HduInfo::ImageInfo { shape, image_type } => {
            let integer = !matches!(image_type, ImageType::Float | ImageType::Double);
            let npix = if shape.is_empty() {
                0
            } else {
                shape.iter().product()
            };
            Ok((npix, integer))
        }
        _ => Err(AtfitsError::Other("primary HDU is not an image".into())),
    }
}

fn check_range(start: usize, end: usize, npix: usize) -> Result<()> {
    if start > end || end > npix {
        return Err(AtfitsError::Other(format!(
            "pixel range {start}..{end} is outside the image ({npix} pixels)"
        )));
    }
    Ok(())
}

macro_rules! impl_cube_elem {
    ($t:ty, $bitpix:expr, $code:expr) => {
        impl CubeElem for $t {
            const BITPIX: i64 = $bitpix;

            fn read_full(fptr: &mut FitsFile) -> Result<Vec<Self>> {
                let (npix, _) = primary_image(fptr)?;
                Self::read_section(fptr, 0, npix)
            }

            fn read_section(fptr: &mut FitsFile, start: usize, end: usize) -> Result<Vec<Self>> {
                let (npix, _) = primary_image(fptr)?;
                check_range(start, end, npix)?;
                let n = end - start;
                let mut out: Vec<$t> = vec![0.0; n];
                let mut nulval: $t = <$t>::NAN;
                let mut anynul: c_int = 0;
                let mut status = 0;
                unsafe {
                    fitsio::sys::ffgpv(
                        fptr.as_raw(),
                        $code as c_int,
                        start as LONGLONG + 1,
                        n as LONGLONG,
                        &mut nulval as *mut $t as *mut c_void,
                        out.as_mut_ptr() as *mut c_void,
                        &mut anynul,
                        &mut status,
                    );
                }
                check_status(status)?;
                Ok(out)
            }

            fn write_section(
                fptr: &mut FitsFile,
                start: usize,
                end: usize,
                data: &[Self],
            ) -> Result<()> {
                let (npix, integer) = primary_image(fptr)?;
                check_range(start, end, npix)?;
                if data.len() != end - start {
                    return Err(AtfitsError::Other(format!(
                        "pixel range {start}..{end} needs {} values, got {}",
                        end - start,
                        data.len()
                    )));
                }
                let mut status = 0;
                if integer && data.iter().any(|v| v.is_nan()) {
                    // cfitsio matches nulls by equality, which never holds for NaN.
                    let mut nulval: $t = <$t>::NEG_INFINITY;
                    let mut buf: Vec<$t> = data
                        .iter()
                        .map(|&v| if v.is_nan() { nulval } else { v })
                        .collect();
                    unsafe {
                        fitsio::sys::ffppn(
                            fptr.as_raw(),
                            $code as c_int,
                            start as LONGLONG + 1,
                            buf.len() as LONGLONG,
                            buf.as_mut_ptr() as *mut c_void,
                            &mut nulval as *mut $t as *mut c_void,
                            &mut status,
                        );
                    }
                } else {
                    unsafe {
                        fitsio::sys::ffppr(
                            fptr.as_raw(),
                            $code as c_int,
                            start as LONGLONG + 1,
                            data.len() as LONGLONG,
                            data.as_ptr() as *mut c_void,
                            &mut status,
                        );
                    }
                }
                check_status(status)?;
                Ok(())
            }
        }
    };
}
impl_cube_elem!(f32, -32, fitsio::sys::TFLOAT);
impl_cube_elem!(f64, -64, fitsio::sys::TDOUBLE);
