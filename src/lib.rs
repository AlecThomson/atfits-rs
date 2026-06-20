//! `atfits-rs` — shared low-level cfitsio helpers for the at*-rs FITS tools
//! (`fitscube-rs`, `convolve-rs`, …).
//!
//! These are the project-agnostic mechanics that every tool needs and that are
//! easy to get subtly wrong against cfitsio:
//!
//! * [`PixelType`] / [`CubeElem`] — precision + monomorphic section I/O.
//! * [`keys`] — update-in-place keyword editing (`ffuky*`, not the duplicating
//!   `ffpky*`) and convenience readers.
//! * [`header`] — [`HeaderGeom`] and WCS [`find_target_axis`] lookup.
//! * [`image`] — image-HDU creation/resize and header-only copying, including
//!   [`create_cube_open`] (the single-open-handle pattern that avoids a doubled
//!   zero-fill pass on close).
//! * [`mem_header`] — build a cube header in memory ([`create_mem_cube`]) and
//!   serialise it ([`extract_header_layout`]) so a tool can write the data unit
//!   with raw I/O and skip cfitsio's zero-fill entirely.
//! * [`output_path`] — output-path construction.
//!
//! Domain logic (beams, combine/extract, convolution, spectral specs) stays in
//! the individual crates.
pub mod error;
pub mod header;
pub mod image;
pub mod keys;
pub mod mem_header;
pub mod path;
pub mod pixel;

pub use error::{AtfitsError, Result};
pub use header::{HeaderGeom, TargetAxis, find_target_axis};
pub use image::{
    bitpix_to_image_type, copy_header_only, copy_header_only_open, create_cube_open,
    is_structural_keyword, resize_image,
};
pub use keys::{
    delete_key, has_key, read_key_f64, read_key_string, update_key_f64, update_key_i64,
    update_key_logical, update_key_str, write_comment,
};
pub use mem_header::{CubeLayout, create_mem_cube, extract_header_layout};
pub use path::output_path;
pub use pixel::{CubeElem, PixelType};
