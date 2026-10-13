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

#[test]
fn template_copy_keeps_blank_only_for_integer_output() {
    let dir = tempfile::tempdir().unwrap();
    let tmpl = int_cube(dir.path());
    for bitpix in [16, -32] {
        let disk = dir.path().join(format!("disk{bitpix}.fits"));
        drop(create_cube_open(&tmpl, &disk, bitpix, &[3, 2, 2]).unwrap());
        let mut mem = create_mem_cube(&tmpl, bitpix, &[3, 2, 2]).unwrap();
        let layout = extract_header_layout(&mut mem, &[3, 2, 2]).unwrap();
        let mem_text = String::from_utf8(layout.header).unwrap();

        let integer = bitpix > 0;
        assert_eq!(read_key_f64(&disk, "BLANK").unwrap().is_some(), integer);
        assert_eq!(read_key_f64(&disk, "BZERO").unwrap().is_some(), integer);
        assert_eq!(mem_text.contains("BLANK   ="), integer);
        assert!(!has_key(&disk, "CHECKSUM").unwrap());
        assert!(!has_key(&disk, "DATASUM").unwrap());
        assert!(!mem_text.contains("CHECKSUM") && !mem_text.contains("DATASUM"));
    }
}

#[test]
fn copied_integer_cube_applies_scaling_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let tmpl = int_cube(dir.path());
    let out = dir.path().join("out.fits");
    {
        let mut f = create_cube_open(&tmpl, &out, 16, &[3, 2, 2]).unwrap();
        f32::write_section(&mut f, 0, 2, &[14.0, f32::NAN]).unwrap();
    }
    let data = f32::read_section(&mut open(&out), 0, 2).unwrap();
    assert_eq!(data[0], 14.0);
    assert!(data[1].is_nan());
}

#[test]
fn copy_header_only_sizes_data_unit_without_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let tmpl = int_cube(dir.path());
    let out = dir.path().join("hdr.fits");
    copy_header_only(&tmpl, &out).unwrap();

    let len = std::fs::metadata(&out).unwrap().len();
    assert_eq!(len % 2880, 0);
    assert!(!has_key(&out, "CHECKSUM").unwrap());
    assert_eq!(read_key_f64(&out, "BLANK").unwrap(), Some(-32768.0));
    assert_eq!(
        read_key_string(&out, "CTYPE3").unwrap().as_deref(),
        Some("FREQ")
    );

    let mut f = copy_header_only_open(&tmpl, &out).unwrap();
    for i in 0..30 {
        update_key_i64(&mut f, &format!("EXTRA{i}"), i).unwrap();
    }
    f32::write_section(&mut f, 0, 2, &[12.0, f32::NAN]).unwrap();
    drop(f);
    assert_eq!(std::fs::metadata(&out).unwrap().len(), len);
    let data = f32::read_full(&mut open(&out)).unwrap();
    assert_eq!(data.len(), 12);
    assert_eq!(data[0], 12.0);
    assert!(data[1].is_nan());
    // Unwritten raw zeros decode through BZERO.
    assert_eq!(data[2], 10.0);
}

#[test]
fn refuses_to_overwrite_input() {
    let dir = tempfile::tempdir().unwrap();
    let tmpl = float_image(dir.path(), "img.fits");
    assert!(copy_header_only(&tmpl, &tmpl).is_err());

    #[cfg(unix)]
    {
        let link = dir.path().join("link.fits");
        std::os::unix::fs::symlink(&tmpl, &link).unwrap();
        assert!(copy_header_only(&tmpl, &link).is_err());
    }

    // On a case-insensitive filesystem (macOS default) IMG.fits is img.fits.
    let upper = dir.path().join("IMG.fits");
    if upper.exists() {
        assert!(copy_header_only(&tmpl, &upper).is_err());
        assert!(create_cube_open(&tmpl, &upper, -32, &[2, 2]).is_err());
    }
    assert_eq!(
        f32::read_full(&mut open(&tmpl)).unwrap(),
        vec![1.0, 2.0, 3.0, 4.0]
    );
}

#[test]
fn mem_header_has_one_end_card_for_any_card_count() {
    let dir = tempfile::tempdir().unwrap();
    let tmpl = float_image(dir.path(), "t.fits");
    let mut f = edit(&tmpl);
    for n in 0..40 {
        update_key_i64(&mut f, &format!("K{n}"), n).unwrap();
        f.primary_hdu().unwrap();
        let mut status = 0;
        unsafe { fitsio::sys::ffflus(f.as_raw(), &mut status) };
        let mut mem = create_mem_cube(&tmpl, -32, &[2, 2]).unwrap();
        let layout = extract_header_layout(&mut mem, &[2, 2]).unwrap();
        let cards: Vec<&[u8]> = layout.header.chunks(80).collect();
        let end = cards.iter().position(|c| c.starts_with(b"END ")).unwrap();
        assert_eq!(layout.header.len() as u64, layout.datastart);
        assert_eq!(layout.datastart % 2880, 0);
        assert!(
            cards[end + 1..]
                .iter()
                .all(|c| c.iter().all(|&b| b == b' '))
        );
    }
}

#[test]
fn update_key_f64_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = float_image(dir.path(), "k.fits");
    let values = [1.234_567_890_123_456_7e9, 0.1 + 0.2, 0.1, -1.0e-300, 5.0];
    {
        let mut f = edit(&path);
        for (i, v) in values.iter().enumerate() {
            update_key_f64(&mut f, &format!("V{i}"), *v).unwrap();
        }
    }
    for (i, v) in values.iter().enumerate() {
        assert_eq!(read_key_f64(&path, &format!("V{i}")).unwrap(), Some(*v));
    }
}

#[test]
fn read_key_distinguishes_missing_from_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let path = float_image(dir.path(), "k.fits");
    update_key_str(&mut edit(&path), "WORD", "abc").unwrap();
    assert_eq!(read_key_f64(&path, "NOPE").unwrap(), None);
    assert_eq!(read_key_string(&path, "NOPE").unwrap(), None);
    assert!(read_key_f64(&path, "WORD").is_err());
}
