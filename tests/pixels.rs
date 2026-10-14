use std::path::{Path, PathBuf};

use atfits_rs::*;
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

fn image(dir: &Path, name: &str, data_type: ImageType, c_dims: &[usize]) -> (PathBuf, FitsFile) {
    let path = dir.join(name);
    let desc = ImageDescription {
        data_type,
        dimensions: c_dims,
    };
    let f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .open()
        .unwrap();
    (path, f)
}

#[test]
fn float_nan_and_buffer_reads() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut f) = image(dir.path(), "f.fits", ImageType::Float, &[2, 2, 2]);
    let data: Vec<f32> = (0..8)
        .map(|i| if i == 5 { f32::NAN } else { i as f32 })
        .collect();
    f32::write_section(&mut f, 0, 8, &data).unwrap();

    let all = f32::read_full(&mut f).unwrap();
    assert!(all[5].is_nan());
    assert_eq!(all[6], 6.0);

    let mut buf = [0.0f32; 4];
    read_plane_into(&mut f, 1, &mut buf).unwrap();
    assert!(buf[1].is_nan());
    assert_eq!(buf[3], 7.0);
    let mut buf = [0.0f64; 3];
    f64::read_section_into(&mut f, 6, &mut buf).unwrap_err();
    f64::read_section_into(&mut f, 0, &mut buf).unwrap();
    assert_eq!(buf, [0.0, 1.0, 2.0]);
}

#[test]
fn raw_integer_io_and_subsets() {
    let dir = tempfile::tempdir().unwrap();
    // NAXIS1=4, NAXIS2=3, NAXIS3=2
    let (_, mut f) = image(dir.path(), "i.fits", ImageType::Short, &[2, 3, 4]);
    assert_eq!(image_shape(&mut f).unwrap(), vec![4, 3, 2]);
    update_key_f64(&mut f, "BSCALE", 2.0).unwrap();
    update_key_f64(&mut f, "BZERO", 1.0).unwrap();
    update_key_i64(&mut f, "BLANK", -1).unwrap();
    unsafe {
        let mut status = 0;
        fitsio::sys::ffrdef(f.as_raw(), &mut status);
    }
    disable_scaling(&mut f).unwrap();
    let stored: Vec<i16> = (0..24).collect();
    write_pixels(&mut f, 0, &stored).unwrap();
    assert!(write_pixels(&mut f, 20, &stored[..5]).is_err());
    write_pixels(&mut f, 23, &[-1i16]).unwrap();

    let mut out = Vec::new();
    read_pixels::<i16>(&mut f, None, None, &mut out).unwrap();
    assert_eq!(out[..23], stored[..23]);
    assert_eq!(out[23], -1);

    // Columns 1..3, row 2, both planes.
    let ranges = [(1, 3), (2, 3), (0, 2)];
    read_pixels::<i16>(&mut f, Some(&ranges), None, &mut out).unwrap();
    assert_eq!(out, vec![9, 10, 21, 22]);
    let mut buf = [0i16; 4];
    read_pixels_into(&mut f, Some(&ranges), None, &mut buf).unwrap();
    assert_eq!(buf, [9, 10, 21, 22]);
    assert!(read_pixels_into(&mut f, Some(&ranges), None, &mut [0i16; 3]).is_err());
    assert!(read_pixels::<i16>(&mut f, Some(&[(0, 5), (0, 3), (0, 2)]), None, &mut out).is_err());
    assert!(read_pixels::<i16>(&mut f, Some(&[(0, 4)]), None, &mut out).is_err());
    drop(f);

    // A fresh handle applies the scaling and maps BLANK to the null value.
    let mut f = FitsFile::open(dir.path().join("i.fits")).unwrap();
    let mut scaled = Vec::new();
    read_pixels::<f32>(
        &mut f,
        Some(&[(0, 4), (0, 1), (1, 2)]),
        Some(f32::NAN),
        &mut scaled,
    )
    .unwrap();
    assert_eq!(scaled, vec![25.0, 27.0, 29.0, 31.0]);
    read_pixels::<f32>(&mut f, None, Some(f32::NAN), &mut scaled).unwrap();
    assert!(scaled[23].is_nan());
}

#[test]
fn big_endian_bytes() {
    let mut ints = [1i16, 2];
    swap_to_be(&mut ints);
    assert_eq!(as_bytes(&ints), &[0, 1, 0, 2]);
    let mut floats = [1.0f32];
    swap_to_be(&mut floats);
    assert_eq!(as_bytes(&floats), &[0x3f, 0x80, 0, 0]);
    assert_eq!(<i64 as Pixel>::BITPIX, 64);
}

#[test]
fn copy_extension_header_to_primary() {
    use fitsio::tables::{ColumnDataType, ColumnDescription};
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("mef.fits");
    let mut src = FitsFile::create(&src_path).open().unwrap();
    let desc = ImageDescription {
        data_type: ImageType::Short,
        dimensions: &[3, 4],
    };
    src.create_image("SCI", &desc).unwrap();
    update_key_str(&mut src, "CTYPE1", "RA---SIN").unwrap();
    write_pixels(&mut src, 0, &[7i16; 12]).unwrap();
    unsafe {
        let mut status = 0;
        fitsio::sys::ffpcks(src.as_raw(), &mut status);
    }
    let col = ColumnDescription::new("X")
        .with_type(ColumnDataType::Int)
        .create()
        .unwrap();
    src.create_table("TAB", &[col]).unwrap();

    let out_path = dir.path().join("out.fits");
    src.hdu(1).unwrap();
    let mut out = copy_hdu_header(&mut src, &out_path).unwrap();
    assert_eq!(image_shape(&mut out).unwrap(), vec![4, 3]);
    write_pixels(&mut out, 4, &[5i16; 4]).unwrap();
    drop(out);

    let bytes = std::fs::read(&out_path).unwrap();
    assert!(bytes.starts_with(b"SIMPLE  =                    T"));
    let text = String::from_utf8_lossy(&bytes);
    for key in ["XTENSION", "PCOUNT", "GCOUNT", "CHECKSUM", "DATASUM"] {
        assert!(!text.contains(key), "{key} copied");
    }
    let mut f = FitsFile::open(&out_path).unwrap();
    assert_eq!(
        read_key_string(&out_path, "CTYPE1").unwrap().unwrap(),
        "RA---SIN"
    );
    let mut px = Vec::new();
    read_pixels::<i16>(&mut f, None, None, &mut px).unwrap();
    assert_eq!(px, [0, 0, 0, 0, 5, 5, 5, 5, 0, 0, 0, 0]);

    src.hdu(2).unwrap();
    assert!(copy_hdu_header(&mut src, &dir.path().join("t.fits")).is_err());
    src.hdu(1).unwrap();
    assert!(copy_hdu_header(&mut src, &src_path).is_err());
}
