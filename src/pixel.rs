//! Pixel precision and section I/O on the primary image.
use std::os::raw::{c_int, c_void};

use fitsio::FitsFile;
use fitsio::hdu::HduInfo;
use fitsio::images::ImageType;
use fitsio::sys::{self, LONGLONG};

use crate::error::{AtfitsError, Result, ffcall};
use crate::raw::read_flat;

/// Precision a cube is processed in.
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

/// Element types (`f32`, `f64`) for reading and writing pixel ranges.
///
/// Ranges are flat pixel offsets `start..end` into the primary image. Integer
/// images are scaled by BSCALE/BZERO; BLANK pixels read as NaN, and NaN is
/// written as BLANK (an error if the header has no BLANK).
pub trait CubeElem: Copy + Send + Sync + 'static {
    /// BITPIX of a float image of this type.
    const BITPIX: i64;
    /// Read the whole primary image.
    fn read_full(fptr: &mut FitsFile) -> Result<Vec<Self>>;
    /// Read pixels `start..end`.
    fn read_section(fptr: &mut FitsFile, start: usize, end: usize) -> Result<Vec<Self>>;
    /// Read pixels `start..start + out.len()` into `out`.
    fn read_section_into(fptr: &mut FitsFile, start: usize, out: &mut [Self]) -> Result<()> {
        let data = Self::read_section(fptr, start, start + out.len())?;
        out.copy_from_slice(&data);
        Ok(())
    }
    /// Write `data` to pixels `start..end`; `data.len()` must equal `end - start`.
    fn write_section(fptr: &mut FitsFile, start: usize, end: usize, data: &[Self]) -> Result<()>;
}

/// Read plane `index` of the primary image, where a plane is `plane_len` pixels.
pub fn read_plane<T: CubeElem>(
    fptr: &mut FitsFile,
    plane_len: usize,
    index: usize,
) -> Result<Vec<T>> {
    let (start, end) = plane_range(plane_len, index)?;
    T::read_section(fptr, start, end)
}

/// Read plane `index` of the primary image into `out`, whose length is the
/// plane size.
pub fn read_plane_into<T: CubeElem>(
    fptr: &mut FitsFile,
    index: usize,
    out: &mut [T],
) -> Result<()> {
    let (start, _) = plane_range(out.len(), index)?;
    T::read_section_into(fptr, start, out)
}

/// Write `data` as plane `index` of the primary image.
pub fn write_plane<T: CubeElem>(fptr: &mut FitsFile, index: usize, data: &[T]) -> Result<()> {
    let (start, end) = plane_range(data.len(), index)?;
    T::write_section(fptr, start, end, data)
}

fn plane_range(plane_len: usize, index: usize) -> Result<(usize, usize)> {
    plane_len
        .checked_mul(index)
        .and_then(|start| Some((start, start.checked_add(plane_len)?)))
        .ok_or_else(|| AtfitsError::Other(format!("plane {index} overflows usize")))
}

/// Pixel count of the primary image and whether it stores integers.
fn primary_image(fptr: &mut FitsFile) -> Result<(usize, bool)> {
    match fptr.primary_hdu()?.info {
        HduInfo::ImageInfo { shape, image_type } => {
            let npix = if shape.is_empty() {
                0
            } else {
                shape.iter().product()
            };
            let integer = !matches!(image_type, ImageType::Float | ImageType::Double);
            Ok((npix, integer))
        }
        _ => Err(AtfitsError::Other("primary HDU is not an image".into())),
    }
}

fn check_range(start: usize, end: usize, npix: usize) -> Result<()> {
    if start > end || end > npix {
        return Err(range_error(start, end, Some(npix)));
    }
    Ok(())
}

fn range_error(start: usize, end: usize, npix: Option<usize>) -> AtfitsError {
    match npix {
        Some(n) => AtfitsError::Other(format!(
            "pixel range {start}..{end} is outside the image ({n} pixels)"
        )),
        None => AtfitsError::Other(format!("pixel range {start}..{end} is reversed")),
    }
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
                if start > end {
                    return Err(range_error(start, end, None));
                }
                let mut out: Vec<$t> = vec![0.0; end - start];
                Self::read_section_into(fptr, start, &mut out)?;
                Ok(out)
            }

            fn read_section_into(
                fptr: &mut FitsFile,
                start: usize,
                out: &mut [Self],
            ) -> Result<()> {
                let (npix, integer) = primary_image(fptr)?;
                let end = start
                    .checked_add(out.len())
                    .ok_or_else(|| range_error(start, usize::MAX, Some(npix)))?;
                check_range(start, end, npix)?;
                // Float NaN reads as NaN anyway; a null value only costs time there.
                read_flat(fptr, start, integer.then_some(<$t>::NAN), out)
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
                if !(integer && data.iter().any(|v| v.is_nan())) {
                    // cfitsio only reads `data`; the C API lacks const.
                    return ffcall(|s| unsafe {
                        sys::ffppr(
                            fptr.as_raw(),
                            $code as c_int,
                            start as LONGLONG + 1,
                            data.len() as LONGLONG,
                            data.as_ptr() as *mut c_void,
                            s,
                        )
                    });
                }
                // cfitsio finds nulls by equality, which never holds for NaN.
                let mut nulval = <$t>::NEG_INFINITY;
                let mut buf: Vec<$t> = data
                    .iter()
                    .map(|&v| if v.is_nan() { nulval } else { v })
                    .collect();
                ffcall(|s| unsafe {
                    sys::ffppn(
                        fptr.as_raw(),
                        $code as c_int,
                        start as LONGLONG + 1,
                        buf.len() as LONGLONG,
                        buf.as_mut_ptr() as *mut c_void,
                        &mut nulval as *mut $t as *mut c_void,
                        s,
                    )
                })
            }
        }
    };
}
impl_cube_elem!(f32, -32, sys::TFLOAT);
impl_cube_elem!(f64, -64, sys::TDOUBLE);
