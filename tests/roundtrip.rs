//! End-to-end sanity: create a cube from a 2D template, stream planes via the
//! `CubeElem` section I/O, and read them back.
use std::path::PathBuf;

use atfits_rs::{CubeElem, PixelType, create_cube_open, update_key_f64};
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

fn template(dir: &std::path::Path, size: usize) -> PathBuf {
    let path = dir.join("template.fits");
    let desc = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[size, size],
    };
    let mut f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .overwrite()
        .open()
        .unwrap();
    let hdu = f.primary_hdu().unwrap();
    hdu.write_image(&mut f, &vec![0.0f32; size * size]).unwrap();
    hdu.write_key(&mut f, "REFFREQ", 1.0e9).unwrap();
    path
}

#[test]
fn create_cube_and_roundtrip_planes() {
    assert_eq!(PixelType::from_bitpix(-64), PixelType::F64);
    assert_eq!(PixelType::from_bitpix(-32), PixelType::F32);

    let dir = tempfile::tempdir().unwrap();
    let size = 8;
    let nchan = 5;
    let tmpl = template(dir.path(), size);
    let cube = dir.path().join("cube.fits");

    let plane_elems = size * size;
    let dims = vec![size, size, nchan];

    {
        let mut out = create_cube_open(&tmpl, &cube, -32, &dims).unwrap();
        // Non-structural template card must have carried over.
        update_key_f64(&mut out, "CRVAL3", 1.0e9).unwrap();
        for c in 0..nchan {
            let plane: Vec<f32> = (0..plane_elems).map(|i| (c * 100 + i) as f32).collect();
            let start = c * plane_elems;
            f32::write_section(&mut out, start, start + plane_elems, &plane).unwrap();
        }
    }

    let mut f = FitsFile::open(cube.to_str().unwrap()).unwrap();
    let all = f32::read_full(&mut f).unwrap();
    assert_eq!(all.len(), plane_elems * nchan);
    for c in 0..nchan {
        for i in 0..plane_elems {
            assert_eq!(all[c * plane_elems + i], (c * 100 + i) as f32);
        }
    }
    let hdu = f.primary_hdu().unwrap();
    let reffreq: f64 = hdu.read_key(&mut f, "REFFREQ").unwrap();
    assert_eq!(reffreq, 1.0e9);
}
