use atfits_rs::*;
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

fn image(dir: &std::path::Path) -> FitsFile {
    let desc = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[2, 2, 2],
    };
    FitsFile::create(dir.join("w.fits"))
        .with_custom_primary(&desc)
        .open()
        .unwrap()
}

#[test]
fn target_axis_applies_pc() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = image(dir.path());
    update_key_str(&mut f, "CTYPE3", "FREQ").unwrap();
    update_key_f64(&mut f, "CDELT3", 2.0e6).unwrap();
    update_key_f64(&mut f, "PC3_3", 0.5).unwrap();
    assert_eq!(find_target_axis_in(&mut f, "FREQ").unwrap().cdelt, 1.0e6);
    update_key_f64(&mut f, "CD3_3", 3.0e6).unwrap();
    assert_eq!(find_target_axis_in(&mut f, "FREQ").unwrap().cdelt, 3.0e6);
}

#[test]
fn unit_scale_depends_on_axis_type() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = image(dir.path());
    update_key_str(&mut f, "CTYPE3", "FREQ").unwrap();
    update_key_str(&mut f, "CUNIT3", "s").unwrap();
    let mut axis = find_target_axis_in(&mut f, "FREQ").unwrap();
    assert_eq!(axis.unit_scale(), None);
    axis.cunit = Some("GHz".into());
    assert_eq!(axis.unit_scale(), Some(1e9));
    axis.ctype = "TIME".into();
    assert_eq!(axis.unit_scale(), None);
    axis.cunit = Some("min".into());
    assert_eq!(axis.unit_scale(), Some(60.0));

    assert_eq!(frequency_unit_scale(None), Some(1.0));
    assert_eq!(frequency_unit_scale(Some("MHZ")), Some(1e6));
    assert_eq!(frequency_unit_scale(Some("h")), None);
    assert_eq!(time_unit_scale(Some("d")), Some(86400.0));
    assert_eq!(time_unit_scale(Some("kHz")), None);
    assert_eq!(spectral_unit_scale(Some("h")), Some(3600.0));
}

#[test]
fn pixel_scale_from_cdelt_pc_or_cd() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = image(dir.path());
    assert!(matches!(
        pixel_scale_deg(&mut f),
        Err(AtfitsError::MissingKeyword(_))
    ));

    update_key_f64(&mut f, "CDELT1", -2.0 / 3600.0).unwrap();
    update_key_f64(&mut f, "CDELT2", 3.0 / 3600.0).unwrap();
    let (dx, dy) = pixel_scale_deg(&mut f).unwrap();
    assert!((dx - 2.0 / 3600.0).abs() < 1e-15 && (dy - 3.0 / 3600.0).abs() < 1e-15);

    update_key_str(&mut f, "CUNIT1", "arcsec").unwrap();
    update_key_f64(&mut f, "CDELT1", -2.0).unwrap();
    let (dx, _) = pixel_scale_deg(&mut f).unwrap();
    assert!((dx - 2.0 / 3600.0).abs() < 1e-15);
    update_key_str(&mut f, "CUNIT1", "m").unwrap();
    assert!(pixel_scale_deg(&mut f).is_err());
    delete_key(&mut f, "CUNIT1").unwrap();

    // Rotated CD matrix with 4 arcsec pixels; CDELT is ignored.
    let (a, t) = (4.0 / 3600.0, 0.3f64);
    update_key_f64(&mut f, "CD1_1", -a * t.cos()).unwrap();
    update_key_f64(&mut f, "CD1_2", a * t.sin()).unwrap();
    update_key_f64(&mut f, "CD2_1", a * t.sin()).unwrap();
    update_key_f64(&mut f, "CD2_2", a * t.cos()).unwrap();
    let (dx, dy) = pixel_scale_deg(&mut f).unwrap();
    assert!((dx - a).abs() < 1e-15 && (dy - a).abs() < 1e-15);
}

#[test]
fn key_bool_reads_logicals_and_strings() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = image(dir.path());
    update_key_logical(&mut f, "L", true).unwrap();
    update_key_logical(&mut f, "LF", false).unwrap();
    update_key_str(&mut f, "S", "TRUE").unwrap();
    update_key_str(&mut f, "SF", "f").unwrap();
    update_key_i64(&mut f, "N", 3).unwrap();
    assert_eq!(key_bool(&mut f, "L").unwrap(), Some(true));
    assert_eq!(key_bool(&mut f, "LF").unwrap(), Some(false));
    assert_eq!(key_bool(&mut f, "S").unwrap(), Some(true));
    assert_eq!(key_bool(&mut f, "SF").unwrap(), Some(false));
    assert_eq!(key_bool(&mut f, "NOPE").unwrap(), None);
    assert!(key_bool(&mut f, "N").is_err());
}
