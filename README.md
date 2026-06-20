# atfits-rs

[![CI](https://github.com/AlecThomson/atfits-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/AlecThomson/atfits-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/atfits-rs.svg)](https://crates.io/crates/atfits-rs)
[![docs.rs](https://img.shields.io/docsrs/atfits-rs)](https://docs.rs/atfits-rs)

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

## Install

```toml
[dependencies]
atfits-rs = "1"
```

## Features

- `fitsio-src` — compile cfitsio from source (needs only a C compiler). Off by
  default; consumers that build wheels enable it. Without it, the system cfitsio
  is linked.

  ```toml
  atfits-rs = { version = "1", features = ["fitsio-src"] }
  ```

## Example

Create a cube from a 2D template, stream planes via `CubeElem` section I/O, then
read them back:

```rust
use atfits_rs::{CubeElem, create_cube_open, update_key_f64};
use fitsio::FitsFile;

let dims = vec![size, size, nchan]; // FITS axis order: NAXIS1, NAXIS2, NAXIS3
let plane_elems = size * size;

// Single open handle — writes the data unit once, no doubled zero-fill on close.
let mut out = create_cube_open(&template, &cube_path, -32, &dims)?;
update_key_f64(&mut out, "CRVAL3", 1.0e9)?; // ffuky* — updates in place

for c in 0..nchan {
    let plane: Vec<f32> = (0..plane_elems).map(|i| (c * 100 + i) as f32).collect();
    let start = c * plane_elems;
    f32::write_section(&mut out, start, start + plane_elems, &plane)?;
}

// Read back.
let mut f = FitsFile::open(cube_path.to_str().unwrap())?;
let all = f32::read_full(&mut f)?;
# Ok::<(), atfits_rs::AtfitsError>(())
```

## Docs

Full API reference: <https://docs.rs/atfits-rs>. Build locally with:

```sh
cargo doc --no-deps --features fitsio-src --open
```

## Contributing

Lint and formatting run via [pre-commit](https://pre-commit.com) hooks
(`cargo fmt`, `cargo clippy`, plus generic checks). CI runs the same hooks with
[prek](https://github.com/j178/prek), a fast Rust reimplementation — install
either:

```sh
# prek (Rust, recommended)
cargo install prek
prek install            # install the git hook
prek run --all-files    # run against the whole tree

# or pre-commit (Python)
pip install pre-commit
pre-commit install
pre-commit run --all-files
```

## License

BSD-3-Clause.
