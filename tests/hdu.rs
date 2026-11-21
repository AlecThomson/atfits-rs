use atfits_rs::*;
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType};

#[test]
fn offsets_count_bitpix_history_and_cards() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cube.fits");
    let desc = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[2, 3, 4],
    };
    let mut f = FitsFile::create(&path)
        .with_custom_primary(&desc)
        .open()
        .unwrap();
    write_plane(&mut f, 1, &[1.5f32; 12]).unwrap();
    write_history(&mut f, "made by a test").unwrap();
    update_key_str(&mut f, "BUNIT", "Jy/beam").unwrap();

    assert_eq!(hdu_count(&mut f).unwrap(), 1);
    assert_eq!(image_bitpix(&mut f).unwrap(), -32);
    assert!(!is_compressed_image(&mut f).unwrap());

    let cards = header_cards(&mut f, &["BUNIT"]).unwrap();
    assert_eq!(cards.len() % 80, 0);
    let names: Vec<&str> = cards.chunks(80).map(card_name).collect();
    assert_eq!(&names[..2], &["SIMPLE", "BITPIX"]);
    assert!(names.contains(&"HISTORY"));
    assert!(!names.contains(&"BUNIT") && !names.contains(&"END"));
    drop(f);

    // Plane 1 read straight from the file at the reported offset
    let mut f = FitsFile::open(&path).unwrap();
    let o = hdu_offsets(&mut f).unwrap();
    assert_eq!(o.header_start, 0);
    assert_eq!(o.data_start % 2880, 0);
    assert_eq!(o.data_end, o.data_start + 2880);
    let bytes = std::fs::read(&path).unwrap();
    let plane = o.data_start as usize + 12 * 4;
    assert_eq!(&bytes[plane..plane + 4], &1.5f32.to_be_bytes());
}
