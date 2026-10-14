//! cfitsio helpers shared by the at*-rs FITS tools (`fitscube-rs`,
//! `convolve-rs`).
//!
//! - [`PixelType`], [`CubeElem`]: `f32`/`f64` pixel range I/O with BLANK as NaN.
//! - [`raw`]: pixel I/O in any FITS type, rectangular subsets, scaling off.
//! - [`keys`]: keyword updates that replace cards instead of duplicating them.
//! - [`header`]: image geometry and WCS axis lookup.
//! - [`image`]: image creation, resizing and header copying.
//! - [`mem_header`]: build a cube header in memory and write the data unit
//!   yourself.
//! - [`output_path`]: output file names.
//!
//! # Threads
//!
//! A [`fitsio::FitsFile`] must stay on the thread that opened it. Separate
//! threads may each open their own handle, including to the same file for
//! reading, when cfitsio is reentrant: the `fitsio-src` build is, and
//! [`is_reentrant`] checks a system library. Write a given file from one handle
//! only.
pub mod error;
pub mod header;
pub mod image;
pub mod keys;
pub mod mem_header;
pub mod path;
pub mod pixel;
pub mod raw;

pub use error::{AtfitsError, Result, is_reentrant, status_message};
pub use header::{
    HeaderGeom, TargetAxis, find_target_axis, find_target_axis_in, spectral_unit_scale,
};
pub use image::{
    bitpix_to_image_type, copy_header_only, copy_header_only_open, create_cube_open,
    is_scaling_keyword, is_structural_keyword, resize_image,
};
pub use keys::{
    delete_key, has_key, key_exists, key_f64, key_i64, key_string, read_key_f64, read_key_string,
    update_key_f64, update_key_i64, update_key_logical, update_key_str, write_comment,
};
pub use mem_header::{CubeLayout, create_mem_cube, extract_header_layout};
pub use path::output_path;
pub use pixel::{CubeElem, PixelType, read_plane, read_plane_into, write_plane};
pub use raw::{
    Pixel, as_bytes, disable_scaling, image_shape, read_pixels, read_pixels_into, swap_to_be,
    write_pixels,
};
