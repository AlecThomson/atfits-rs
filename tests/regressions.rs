use std::path::{Path, PathBuf};

use atfits_rs::*;
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

/// 2x2x3 int16 cube with BLANK, BSCALE, BZERO, a FREQ axis and checksums.
fn int_cube(dir: &Path) -> PathBuf {
    let path = dir.join("int.fits");
    let desc = ImageDescription {
        data_type: ImageType::Short,
        dimensions: &[3, 2, 2],
    };
    let mut f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .overwrite()
        .open()
        .unwrap();
    update_key_i64(&mut f, "BLANK", -32768).unwrap();
    update_key_f64(&mut f, "BSCALE", 2.0).unwrap();
    update_key_f64(&mut f, "BZERO", 10.0).unwrap();
    update_key_str(&mut f, "CTYPE3", "FREQ").unwrap();
    update_key_f64(&mut f, "CRVAL3", 1.0e9).unwrap();
    update_key_f64(&mut f, "CD3_3", 1.0e6).unwrap();
    // Re-read so cfitsio applies the scaling keywords added above.
    let mut status = 0;
    unsafe { fitsio::sys::ffrdef(f.as_raw(), &mut status) };
    let hdu = f.primary_hdu().unwrap();
    let raw: Vec<i16> = (0..12).map(|i| if i == 1 { -32768 } else { i }).collect();
    unsafe { fitsio::sys::ffpscl(f.as_raw(), 1.0, 0.0, &mut status) };
    hdu.write_image(&mut f, &raw).unwrap();
    unsafe {
        fitsio::sys::ffpscl(f.as_raw(), 2.0, 10.0, &mut status);
        fitsio::sys::ffpcks(f.as_raw(), &mut status);
    }
    assert_eq!(status, 0);
    path
}

fn float_image(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let desc = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[2, 2],
    };
    let mut f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .overwrite()
        .open()
        .unwrap();
    let hdu = f.primary_hdu().unwrap();
    hdu.write_image(&mut f, &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    path
}

fn open(path: &Path) -> FitsFile {
    FitsFile::open(path.to_str().unwrap()).unwrap()
}

fn edit(path: &Path) -> FitsFile {
    FitsFile::edit(path.to_str().unwrap()).unwrap()
}

#[test]
fn bad_section_ranges_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = float_image(dir.path(), "f.fits");
    let mut f = edit(&path);
    assert!(f32::write_section(&mut f, 0, 4, &[1.0, 2.0]).is_err());
    assert!(f32::write_section(&mut f, 3, 2, &[]).is_err());
    assert!(f32::read_section(&mut f, 3, 2).is_err());
    assert!(f32::read_section(&mut f, 0, 5).is_err());
    assert_eq!(f32::read_section(&mut f, 1, 3).unwrap(), vec![2.0, 3.0]);
}

#[test]
fn blank_pixels_read_as_nan() {
    let dir = tempfile::tempdir().unwrap();
    let path = int_cube(dir.path());
    let data = f32::read_full(&mut open(&path)).unwrap();
    assert!(data[1].is_nan());
    assert_eq!(data[2], 2.0 * 2.0 + 10.0);
    let data = f64::read_section(&mut open(&path), 0, 3).unwrap();
    assert!(data[1].is_nan());
}

#[test]
fn nan_writes_as_blank_in_integer_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = int_cube(dir.path());
    {
        let mut f = edit(&path);
        f32::write_section(&mut f, 4, 6, &[f32::NAN, 30.0]).unwrap();
    }
    let data = f32::read_section(&mut open(&path), 4, 6).unwrap();
    assert!(data[0].is_nan());
    assert_eq!(data[1], 30.0);

    let int = dir.path().join("noblank.fits");
    let desc = ImageDescription {
        data_type: ImageType::Short,
        dimensions: &[2],
    };
    let mut f = FitsFile::create(&int)
        .with_custom_primary(&desc)
        .open()
        .unwrap();
    assert!(f32::write_section(&mut f, 0, 2, &[f32::NAN, 1.0]).is_err());
}
