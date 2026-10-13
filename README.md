# atfits-rs

[![CI](https://github.com/AlecThomson/atfits-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/AlecThomson/atfits-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/atfits-rs.svg)](https://crates.io/crates/atfits-rs)
[![docs.rs](https://img.shields.io/docsrs/atfits-rs)](https://docs.rs/atfits-rs)

[cfitsio](https://heasarc.gsfc.nasa.gov/fitsio/) helpers shared by
[`fitscube-rs`](https://github.com/AlecThomson/fitscube-rs) and
[`convolve-rs`](https://github.com/AlecThomson/convolve-rs):

- `PixelType`, `CubeElem`, `read_plane`, `write_plane`: `f32`/`f64` pixel I/O,
  with BLANK read as NaN and NaN written as BLANK.
- `keys`: keyword updates that replace the existing card (`fitsio`'s
  `write_key` appends a duplicate), and readers that tell a missing keyword
  from a malformed one.
- `header`: image geometry and WCS axis lookup.
- `image`: image creation, resizing and header-only copies.
- `mem_header`: build a cube header in memory and write the data unit with
  plain file I/O.
- `output_path`: output file names.

## Install

```toml
[dependencies]
atfits-rs = "1"
```

The `fitsio-src` feature compiles cfitsio from source, which needs only a C
compiler. Without it the system cfitsio is linked.

```toml
atfits-rs = { version = "1", features = ["fitsio-src"] }
```

## Example

Create a cube from a 2D template, write each plane, then read the cube back:

```rust
use atfits_rs::{CubeElem, create_cube_open, update_key_f64, write_plane};
use fitsio::FitsFile;

let dims = [size, size, nchan]; // NAXIS1, NAXIS2, NAXIS3
let mut out = create_cube_open(&template, &cube_path, -32, &dims)?;
update_key_f64(&mut out, "CRVAL3", 1.0e9)?;
for c in 0..nchan {
    write_plane(&mut out, c, &vec![c as f32; size * size])?;
}
drop(out);

let mut f = FitsFile::open(&cube_path)?;
let all = f32::read_full(&mut f)?;
```

## Threads

A `FitsFile` must stay on the thread that opened it. Threads may each open
their own handle when cfitsio is reentrant, which the `fitsio-src` build is;
`is_reentrant()` checks a system library. Write a given file from one handle
only.

## Development

```sh
cargo test --features fitsio-src
prek run --all-files     # or: pre-commit run --all-files
cargo doc --no-deps --features fitsio-src --open
```

CI runs the same hooks (`cargo fmt`, `cargo clippy`, generic checks) with
[prek](https://github.com/j178/prek).

## License

BSD-3-Clause.
