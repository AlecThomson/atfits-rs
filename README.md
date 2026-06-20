# atfits-rs

Shared low-level [cfitsio](https://heasarc.gsfc.nasa.gov/fitsio/) helpers for the
`at*-rs` family of FITS tools ([`fitscube-rs`](https://github.com/AlecThomson/fitscube-rs),
[`convolve-rs`](https://github.com/AlecThomson/convolve-rs), …).

These are the project-agnostic mechanics that are easy to get subtly wrong
against cfitsio, factored out so each tool shares one tested implementation:

- **`PixelType` / `CubeElem`** — pixel precision (`f32`/`f64`, from `BITPIX`) and
  monomorphic section read/write.
- **`keys`** — update-*in-place* keyword editing via `ffuky*` (cfitsio's `ffpky*`
  appends duplicate cards) plus convenience readers.
- **`header`** — `HeaderGeom` (shape/BITPIX) and WCS `find_target_axis`.
- **`image`** — image-HDU creation/resize and header-only copying, including
  `create_cube_open`: the single-open-handle pattern that writes the data unit
  once instead of a doubled zero-fill pass on close.
- **`output_path`** — output-path construction.

Domain logic (beams, combine/extract, convolution, spectral specs) stays in the
individual crates.

## Features

- `fitsio-src` — compile cfitsio from source (needs only a C compiler). Off by
  default; consumers that build wheels enable it. Without it, the system cfitsio
  is linked.

## License

BSD-3-Clause.
