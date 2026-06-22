//! End-to-end sanity: create a cube from a 2D template, stream planes via the
//! `CubeElem` section I/O, and read them back.
use std::path::PathBuf;

use atfits_rs::{
    CubeElem, PixelType, create_cube_open, create_mem_cube, extract_header_layout, update_key_f64,
};
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

/// The in-memory header builder must report the real cube shape even though it
/// allocates only size-1 dummy axes (so it never materialises a cube-sized data
/// buffer). Build the header, lay it down with raw I/O + a sparse data unit, and
/// confirm cfitsio reads back the intended NAXISn.
#[test]
fn mem_header_reports_real_dims() {
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};

    let dir = tempfile::tempdir().unwrap();
    let size = 8;
    let nchan = 5;
    let tmpl = template(dir.path(), size);
    let cube = dir.path().join("mem_cube.fits");

    let dims = vec![size, size, nchan];
    let mut fptr = create_mem_cube(&tmpl, -32, &dims).unwrap();
    let layout = extract_header_layout(&mut fptr, &dims).unwrap();

    // Header is a whole number of 2880-byte blocks and the data unit starts right
    // after it.
    assert_eq!(layout.datastart % 2880, 0);
    assert_eq!(layout.header.len() as u64, layout.datastart);

    // Lay down the header and sparsely extend to the full (header + data) length.
    let plane_bytes = (size * size * 4) as u64;
    let data_bytes = plane_bytes * nchan as u64;
    let total = layout.datastart + data_bytes.div_ceil(2880) * 2880;
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&cube)
            .unwrap();
        file.write_all(&layout.header).unwrap();
        file.seek(SeekFrom::Start(total - 1)).unwrap();
        file.write_all(&[0]).unwrap();
    }

    // cfitsio must agree on the shape we stamped into the dummy-axis header.
    let mut f = FitsFile::open(cube.to_str().unwrap()).unwrap();
    let hdu = f.primary_hdu().unwrap();
    let n1: i64 = hdu.read_key(&mut f, "NAXIS1").unwrap();
    let n2: i64 = hdu.read_key(&mut f, "NAXIS2").unwrap();
    let n3: i64 = hdu.read_key(&mut f, "NAXIS3").unwrap();
    assert_eq!((n1, n2, n3), (size as i64, size as i64, nchan as i64));
    // Non-structural template card carried over.
    let reffreq: f64 = hdu.read_key(&mut f, "REFFREQ").unwrap();
    assert_eq!(reffreq, 1.0e9);
}
