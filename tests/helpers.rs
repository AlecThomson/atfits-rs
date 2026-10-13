use atfits_rs::*;
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

#[test]
fn status_message_and_reentrancy() {
    assert_eq!(status_message(202), "keyword not found in header");
    #[cfg(feature = "fitsio-src")]
    assert!(is_reentrant());
}

#[test]
fn planes_and_open_file_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cube.fits");
    let desc = ImageDescription {
        data_type: ImageType::Double,
        dimensions: &[3, 2, 2],
    };
    let mut f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .open()
        .unwrap();
    update_key_logical(&mut f, "FLAG", true).unwrap();
    update_key_str(&mut f, "CTYPE3", "FREQ").unwrap();
    for c in 0..3 {
        write_plane(&mut f, c, &[c as f64; 4]).unwrap();
    }
    assert_eq!(read_plane::<f64>(&mut f, 4, 2).unwrap(), vec![2.0; 4]);
    assert!(read_plane::<f64>(&mut f, 4, 3).is_err());

    assert!(key_exists(&mut f, "FLAG").unwrap());
    assert!(!key_exists(&mut f, "NOPE").unwrap());
    assert_eq!(key_i64(&mut f, "NAXIS3").unwrap(), Some(3));

    let geom = HeaderGeom::from_file(&mut f).unwrap();
    assert_eq!((geom.dims.clone(), geom.bitpix), (vec![2, 2, 3], -64));
    assert_eq!(geom.plane_len(), 4);
    assert_eq!(find_target_axis_in(&mut f, "FREQ").unwrap().fits_idx, 3);
    assert!(find_target_axis_in(&mut f, "TIME").is_err());
}
