//! Pixel I/O on the current image HDU in any FITS pixel type, including stored
//! integers with scaling turned off and rectangular subsets.
use std::os::raw::{c_int, c_long, c_void};

use fitsio::FitsFile;
use fitsio::sys::{self, LONGLONG};

use crate::error::{AtfitsError, Result, ffcall};

mod sealed {
    pub trait Sealed {}
}

/// A FITS pixel type: `u8`, `i16`, `i32`, `i64`, `f32` or `f64`.
pub trait Pixel: sealed::Sealed + Copy + Default + Send + Sync + 'static {
    /// cfitsio type code.
    const DATATYPE: c_int;
    /// BITPIX of an image stored as this type.
    const BITPIX: i64;
    /// The value with its bytes in big-endian (FITS) order.
    fn to_be(self) -> Self;
}

macro_rules! impl_pixel {
    ($t:ty, $code:ident, $bitpix:expr, $v:ident => $be:expr) => {
        impl sealed::Sealed for $t {}
        impl Pixel for $t {
            const DATATYPE: c_int = sys::$code as c_int;
            const BITPIX: i64 = $bitpix;
            fn to_be(self) -> Self {
                let $v = self;
                $be
            }
        }
    };
}
impl_pixel!(u8, TBYTE, 8, v => v);
impl_pixel!(i16, TSHORT, 16, v => v.to_be());
impl_pixel!(i32, TINT, 32, v => v.to_be());
impl_pixel!(i64, TLONGLONG, 64, v => v.to_be());
impl_pixel!(f32, TFLOAT, -32, v => f32::from_bits(v.to_bits().to_be()));
impl_pixel!(f64, TDOUBLE, -64, v => f64::from_bits(v.to_bits().to_be()));

/// Swap `data` to big-endian byte order in place, ready for [`as_bytes`].
pub fn swap_to_be<T: Pixel>(data: &mut [T]) {
    for v in data {
        *v = v.to_be();
    }
}

/// View pixels as bytes.
pub fn as_bytes<T: Pixel>(data: &[T]) -> &[u8] {
    // SAFETY: every `Pixel` type is a plain number without padding.
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), std::mem::size_of_val(data)) }
}

/// Make this handle read and write stored values, ignoring BSCALE and BZERO.
///
/// Affects only this handle; the header is unchanged.
pub fn disable_scaling(fptr: &mut FitsFile) -> Result<()> {
    ffcall(|s| unsafe { sys::ffpscl(fptr.as_raw(), 1.0, 0.0, s) })
}

/// Shape of the current image HDU, `dims[0]` = NAXIS1.
pub fn image_shape(fptr: &mut FitsFile) -> Result<Vec<usize>> {
    let mut naxis: c_int = 0;
    ffcall(|s| unsafe { sys::ffgidm(fptr.as_raw(), &mut naxis, s) })?;
    let mut naxes: Vec<LONGLONG> = vec![0; naxis as usize];
    ffcall(|s| unsafe { sys::ffgiszll(fptr.as_raw(), naxis, naxes.as_mut_ptr(), s) })?;
    Ok(naxes.into_iter().map(|n| n as usize).collect())
}

/// Read pixels of the current image HDU into `out`, resized to fit.
///
/// `ranges` gives a 0-based, end-exclusive range per axis (`ranges[0]` is
/// NAXIS1), or `None` for the whole image. With `nulval`, BLANK and NaN pixels
/// read as it; without, values are returned as stored (after any scaling).
pub fn read_pixels<T: Pixel>(
    fptr: &mut FitsFile,
    ranges: Option<&[(usize, usize)]>,
    nulval: Option<T>,
    out: &mut Vec<T>,
) -> Result<()> {
    let shape = image_shape(fptr)?;
    out.clear();
    out.resize(subset_len(&shape, ranges)?, T::default());
    read_checked(fptr, &shape, ranges, nulval, out)
}

/// [`read_pixels`] into a caller buffer, whose length must match the selection.
pub fn read_pixels_into<T: Pixel>(
    fptr: &mut FitsFile,
    ranges: Option<&[(usize, usize)]>,
    nulval: Option<T>,
    out: &mut [T],
) -> Result<()> {
    let shape = image_shape(fptr)?;
    let n = subset_len(&shape, ranges)?;
    if out.len() != n {
        return Err(AtfitsError::Other(format!(
            "buffer holds {} pixels, selection has {n}",
            out.len()
        )));
    }
    read_checked(fptr, &shape, ranges, nulval, out)
}

fn subset_len(shape: &[usize], ranges: Option<&[(usize, usize)]>) -> Result<usize> {
    let Some(ranges) = ranges else {
        return Ok(if shape.is_empty() {
            0
        } else {
            shape.iter().product()
        });
    };
    if ranges.len() != shape.len() {
        return Err(AtfitsError::Other(format!(
            "{} ranges for a {}-axis image",
            ranges.len(),
            shape.len()
        )));
    }
    for (i, (&(a, b), &n)) in ranges.iter().zip(shape).enumerate() {
        if a > b || b > n {
            return Err(AtfitsError::Other(format!(
                "range {a}..{b} is outside NAXIS{}={n}",
                i + 1
            )));
        }
    }
    Ok(ranges.iter().map(|(a, b)| b - a).product())
}

fn read_checked<T: Pixel>(
    fptr: &mut FitsFile,
    shape: &[usize],
    ranges: Option<&[(usize, usize)]>,
    nulval: Option<T>,
    out: &mut [T],
) -> Result<()> {
    let whole = ranges.is_none_or(|r| r.iter().zip(shape).all(|(&(a, b), &n)| a == 0 && b == n));
    if whole {
        return read_flat(fptr, 0, nulval, out);
    }
    if out.is_empty() {
        return Ok(());
    }
    let r = ranges.unwrap_or_default();
    let mut first: Vec<c_long> = r.iter().map(|&(a, _)| a as c_long + 1).collect();
    let mut last: Vec<c_long> = r.iter().map(|&(_, b)| b as c_long).collect();
    let mut inc: Vec<c_long> = vec![1; r.len()];
    let mut nulval = nulval;
    let mut anynul: c_int = 0;
    ffcall(|s| unsafe {
        sys::ffgsv(
            fptr.as_raw(),
            T::DATATYPE,
            first.as_mut_ptr(),
            last.as_mut_ptr(),
            inc.as_mut_ptr(),
            nul_ptr(&mut nulval),
            out.as_mut_ptr().cast(),
            &mut anynul,
            s,
        )
    })
}

fn nul_ptr<T>(nulval: &mut Option<T>) -> *mut c_void {
    nulval
        .as_mut()
        .map_or(std::ptr::null_mut(), |v| (v as *mut T).cast())
}

/// Read `out.len()` pixels from flat offset `start` of the current image.
pub(crate) fn read_flat<T: Pixel>(
    fptr: &mut FitsFile,
    start: usize,
    nulval: Option<T>,
    out: &mut [T],
) -> Result<()> {
    if out.is_empty() {
        return Ok(());
    }
    let mut nulval = nulval;
    let mut anynul: c_int = 0;
    ffcall(|s| unsafe {
        sys::ffgpv(
            fptr.as_raw(),
            T::DATATYPE,
            start as LONGLONG + 1,
            out.len() as LONGLONG,
            nul_ptr(&mut nulval),
            out.as_mut_ptr().cast(),
            &mut anynul,
            s,
        )
    })
}

/// Write `data` at flat pixel offset `start` of the current image HDU.
///
/// Values are converted to the image type and scaled unless
/// [`disable_scaling`] was called.
pub fn write_pixels<T: Pixel>(fptr: &mut FitsFile, start: usize, data: &[T]) -> Result<()> {
    let shape = image_shape(fptr)?;
    let npix = subset_len(&shape, None)?;
    if start.checked_add(data.len()).is_none_or(|end| end > npix) {
        return Err(AtfitsError::Other(format!(
            "{} pixels at offset {start} overrun the image ({npix} pixels)",
            data.len()
        )));
    }
    if data.is_empty() {
        return Ok(());
    }
    // cfitsio only reads `data`; the C API lacks const.
    ffcall(|s| unsafe {
        sys::ffppr(
            fptr.as_raw(),
            T::DATATYPE,
            start as LONGLONG + 1,
            data.len() as LONGLONG,
            data.as_ptr() as *mut c_void,
            s,
        )
    })
}
