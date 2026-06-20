//! Pixel precision and monomorphic cfitsio section I/O.
use fitsio::FitsFile;

use crate::error::Result;

/// Pixel precision a cube is read/written in, derived from FITS `BITPIX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelType {
    F32,
    F64,
}

impl PixelType {
    /// `-64` → f64; everything else (`-32` and integer BITPIX) → f32, matching
    /// the working precision of the original tools.
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
/// Bundles the cfitsio section read/write so a streaming pipeline can be generic
/// over precision while keeping the cfitsio calls monomorphic.
pub trait CubeElem: Copy + Send + Sync + 'static {
    /// FITS `BITPIX` written for this element type.
    const BITPIX: i64;
    fn read_full(fptr: &mut FitsFile) -> Result<Vec<Self>>;
    fn read_section(fptr: &mut FitsFile, start: usize, end: usize) -> Result<Vec<Self>>;
    fn write_section(fptr: &mut FitsFile, start: usize, end: usize, data: &[Self]) -> Result<()>;
}

macro_rules! impl_cube_elem {
    ($t:ty, $bitpix:expr) => {
        impl CubeElem for $t {
            const BITPIX: i64 = $bitpix;
            fn read_full(fptr: &mut FitsFile) -> Result<Vec<Self>> {
                let hdu = fptr.primary_hdu()?;
                Ok(hdu.read_image(fptr)?)
            }
            fn read_section(fptr: &mut FitsFile, start: usize, end: usize) -> Result<Vec<Self>> {
                let hdu = fptr.primary_hdu()?;
                Ok(hdu.read_section(fptr, start, end)?)
            }
            fn write_section(
                fptr: &mut FitsFile,
                start: usize,
                end: usize,
                data: &[Self],
            ) -> Result<()> {
                let hdu = fptr.primary_hdu()?;
                hdu.write_section(fptr, start, end, data)?;
                Ok(())
            }
        }
    };
}
impl_cube_elem!(f32, -32);
impl_cube_elem!(f64, -64);
