//! HDU layout and type queries on the current HDU.
use std::os::raw::c_int;

use fitsio::FitsFile;
use fitsio::sys::{self, LONGLONG};

use crate::error::{Result, ffcall};

/// Byte offsets of the current HDU within its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HduOffsets {
    /// Start of the header.
    pub header_start: u64,
    /// Start of the data unit, after the padded header.
    pub data_start: u64,
    /// End of the padded data unit, where the next HDU starts.
    pub data_end: u64,
}

/// Byte offsets of the current HDU's header and data unit.
///
/// With the data unit uncompressed and unscaled, plane `i` of an image of
/// `plane_bytes` bytes a plane starts at `data_start + i * plane_bytes`, so it
/// can be read with plain file I/O from any thread.
pub fn hdu_offsets(fptr: &mut FitsFile) -> Result<HduOffsets> {
    let (mut head, mut data, mut end): (LONGLONG, LONGLONG, LONGLONG) = (0, 0, 0);
    ffcall(|s| unsafe { sys::ffghadll(fptr.as_raw(), &mut head, &mut data, &mut end, s) })?;
    Ok(HduOffsets {
        header_start: head as u64,
        data_start: data as u64,
        data_end: end as u64,
    })
}

/// Number of HDUs in the file.
pub fn hdu_count(fptr: &mut FitsFile) -> Result<usize> {
    let mut n: c_int = 0;
    ffcall(|s| unsafe { sys::ffthdu(fptr.as_raw(), &mut n, s) })?;
    Ok(n as usize)
}

/// BITPIX of the current image HDU. For a tile-compressed image this is the
/// BITPIX of the image it holds (ZBITPIX), not of the table storing it.
pub fn image_bitpix(fptr: &mut FitsFile) -> Result<i64> {
    let mut bitpix: c_int = 0;
    ffcall(|s| unsafe { sys::ffgidt(fptr.as_raw(), &mut bitpix, s) })?;
    Ok(i64::from(bitpix))
}

/// Whether the current HDU is a tile-compressed image.
pub fn is_compressed_image(fptr: &mut FitsFile) -> Result<bool> {
    let mut status = 0;
    let compressed = unsafe { sys::fits_is_compressed_image(fptr.as_raw(), &mut status) };
    fitsio::errors::check_status(status)?;
    Ok(compressed != 0)
}
