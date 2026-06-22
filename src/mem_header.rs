//! Build a cube's FITS header without touching disk, so the data unit is never
//! zero-filled by cfitsio.
//!
//! cfitsio writes a full pass of zeros over the data unit when an image HDU is
//! flushed/closed (≈0.5 s for a 500 MB cube; it is NOT sparse on APFS). A
//! streaming-write tool can sidestep this the way the Python reference tools do:
//! write only the header, sparsely extend the file to its final length (so the
//! OS backs the data unit with zero pages on demand), then write the real planes
//! exactly once with plain `std::fs` I/O.
//!
//! To get the header bytes without any disk zero-fill, [`create_mem_cube`]
//! assembles the HDU in an **in-memory** cfitsio file (`mem://`) and
//! [`extract_header_layout`] serialises it with `ffhdr2str`. The caller owns the
//! data-unit write (raw `write_all_at` of big-endian planes).
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use fitsio::FitsFile;
use fitsio::errors::check_status;
use fitsio::sys::{LONGLONG, fitsfile};

use crate::error::{AtfitsError, Result};
use crate::image::is_structural_keyword;

/// The byte layout of a freshly built cube: the complete primary header padded
/// to a 2880-byte block, and the byte offset where the data unit begins.
pub struct CubeLayout {
    /// Full primary header, padded with spaces to `datastart` bytes.
    pub header: Vec<u8>,
    /// Byte offset of the data unit (== `header.len()`).
    pub datastart: u64,
}

/// Create an in-memory image HDU with the same number of axes as `dims` (FITS
/// order, NAXIS1 first) and `bitpix`, copy every non-structural card from
/// `template`'s primary header, and return the open (memory-backed) handle.
///
/// Only the axis *count* of `dims` is used here, not the lengths: the HDU is
/// created with size-1 dummy axes so cfitsio never allocates the full
/// (cube-sized) data unit in RAM just to assemble a header. The real axis
/// lengths are stamped into the serialised header by [`extract_header_layout`],
/// which takes the same `dims`. `datastart` is independent of the values (every
/// FITS card is a fixed 80 bytes), so the layout is identical to a full-size HDU.
///
/// Nothing is written to disk; closing the returned handle just frees RAM. Pair
/// with [`extract_header_layout`] to obtain the on-disk header bytes.
pub fn create_mem_cube(template: &Path, bitpix: i64, dims: &[usize]) -> Result<FitsFile> {
    let mut status = 0;
    let memname = CString::new("mem://").expect("static name has no NUL");
    let mut raw: *mut fitsfile = ptr::null_mut();
    unsafe {
        fitsio::sys::ffinit(&mut raw, memname.as_ptr(), &mut status);
    }
    check_status(status)?;

    // `ffcrimll` takes naxes in FITS order (naxes[0] == NAXIS1). Use size-1 dummy
    // axes: a full-size image would make cfitsio allocate (and zero) the entire
    // data unit in memory. `extract_header_layout` patches the real lengths into
    // the NAXISn cards afterwards.
    let mut naxes: Vec<LONGLONG> = vec![1; dims.len()];
    unsafe {
        fitsio::sys::ffcrimll(
            raw,
            bitpix as c_int,
            naxes.len() as c_int,
            naxes.as_mut_ptr(),
            &mut status,
        );
    }
    if let Err(e) = check_status(status) {
        let mut close = 0;
        unsafe { fitsio::sys::ffclos(raw, &mut close) };
        return Err(e.into());
    }

    // Copy every non-structural card from the template's primary header.
    let mut in_fptr = FitsFile::open(template.to_string_lossy().as_ref())?;
    in_fptr.primary_hdu()?;
    unsafe {
        let mut nkeys: c_int = 0;
        let mut morekeys: c_int = 0;
        fitsio::sys::ffghsp(in_fptr.as_raw(), &mut nkeys, &mut morekeys, &mut status);
        check_status(status)?;

        // `c_char` signedness differs by platform; match cfitsio's `*mut c_char`.
        let mut card = [0 as c_char; 81];
        for i in 1..=nkeys {
            card.fill(0);
            fitsio::sys::ffgrec(in_fptr.as_raw(), i, card.as_mut_ptr(), &mut status);
            if check_status(status).is_err() {
                break;
            }
            let card_str = CStr::from_ptr(card.as_ptr()).to_string_lossy();
            let name = card_str.split([' ', '=']).next().unwrap_or("").trim();
            if is_structural_keyword(name) {
                continue;
            }
            fitsio::sys::ffprec(raw, card.as_ptr(), &mut status);
            check_status(status)?;
        }
    }

    let handle = unsafe { FitsFile::from_raw(raw, fitsio::FileOpenMode::READWRITE)? };
    Ok(handle)
}

/// Overwrite the value of the `NAXIS{axis}` card in a serialised FITS header (a
/// run of 80-byte cards) with `value`, preserving the 80-byte card width and any
/// trailing comment. The HDU is built with size-1 dummy axes (see
/// [`create_mem_cube`]); this stamps the real lengths into the header bytes
/// without asking cfitsio to resize the data unit.
fn set_naxis_card(header: &mut [u8], axis: usize, value: i64) -> Result<()> {
    let key = format!("NAXIS{axis}");
    let key_bytes = key.as_bytes();
    for card in header.chunks_exact_mut(80) {
        // The keyword occupies bytes 0..8, blank-padded.
        let kw_end = card[..8].iter().position(|&b| b == b' ').unwrap_or(8);
        if kw_end == key_bytes.len() && &card[..kw_end] == key_bytes {
            // Fixed-format integers are right-justified in columns 11..30 (byte
            // indices 10..30); bytes 30..80 hold any comment.
            let s = value.to_string();
            if s.len() > 20 {
                return Err(AtfitsError::Other(format!(
                    "NAXIS{axis} value {value} does not fit a FITS card"
                )));
            }
            let field = &mut card[10..30];
            field.fill(b' ');
            field[20 - s.len()..].copy_from_slice(s.as_bytes());
            return Ok(());
        }
    }
    Err(AtfitsError::Other(format!(
        "NAXIS{axis} card not found in serialised header"
    )))
}

/// Serialise the primary header of `fptr` to bytes and report where the data
/// unit starts, so the caller can lay the header down with raw I/O.
///
/// `dims` are the real axis lengths (FITS order, NAXIS1 first). They are stamped
/// into the `NAXISn` cards because [`create_mem_cube`] builds the HDU with size-1
/// dummy axes to avoid allocating a cube-sized data unit; pass the same `dims`
/// given to `create_mem_cube`.
pub fn extract_header_layout(fptr: &mut FitsFile, dims: &[usize]) -> Result<CubeLayout> {
    fptr.primary_hdu()?; // position at the primary HDU
    let raw = unsafe { fptr.as_raw() };
    let mut status = 0;

    let mut headstart: LONGLONG = 0;
    let mut datastart: LONGLONG = 0;
    let mut dataend: LONGLONG = 0;
    unsafe {
        fitsio::sys::ffghadll(
            raw,
            &mut headstart,
            &mut datastart,
            &mut dataend,
            &mut status,
        );
    }
    check_status(status)?;

    // `ffhdr2str` concatenates every card (80 chars each) into one malloc'd
    // string, EXCLUDING the END card and the trailing block padding.
    let mut header_ptr: *mut c_char = ptr::null_mut();
    let mut nkeys: c_int = 0;
    unsafe {
        fitsio::sys::ffhdr2str(
            raw,
            0, // keep COMMENT/HISTORY cards
            ptr::null_mut(),
            0,
            &mut header_ptr,
            &mut nkeys,
            &mut status,
        );
    }
    check_status(status)?;

    let mut header = unsafe {
        let bytes = CStr::from_ptr(header_ptr).to_bytes().to_vec();
        let mut free_status = 0;
        fitsio::sys::fffree(header_ptr as *mut c_void, &mut free_status);
        bytes
    };

    let datastart = datastart as u64;
    if header.len() as u64 + 3 > datastart {
        return Err(AtfitsError::Other(format!(
            "header ({} bytes) does not fit before data unit ({datastart} bytes)",
            header.len()
        )));
    }
    // Re-append the END card and pad to the data-unit boundary with spaces — the
    // exact on-disk header cfitsio's own layout (datastart) accounts for.
    header.extend_from_slice(b"END");
    header.resize(datastart as usize, b' ');

    // The HDU was built with size-1 dummy axes (see `create_mem_cube`); stamp the
    // real lengths into the NAXISn cards. Card width is fixed, so `datastart` is
    // unaffected.
    for (i, &d) in dims.iter().enumerate() {
        set_naxis_card(&mut header, i + 1, d as i64)?;
    }

    Ok(CubeLayout { header, datastart })
}
