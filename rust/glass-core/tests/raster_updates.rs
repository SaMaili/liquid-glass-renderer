use liquid_glass_core::raster::{Content, Patch, PixelRect, Scale, Viewport};

#[test]
fn high_density_updates_keep_point_geometry_and_reject_partial_writes() {
    let viewport = Viewport::new(3, 2, Scale::Two).unwrap();
    assert_eq!(viewport.pixel_center(5, 3), Some([2.75, 1.75]));
    assert_eq!(viewport.pixel_center(6, 3), None);
    let mut content = Content::new(viewport, 1, vec![0.0; 96]).unwrap();
    let rect = PixelRect { x: 3, y: 2, width: 2, height: 2 };
    let patch = Patch { rect, rgba: &[1.0; 16] };
    let invalid = Patch { rect: PixelRect { x: 5, ..rect }, rgba: &[1.0; 16] };
    assert!(content.update(2, &[patch, invalid]).is_err());
    assert_eq!(content.version(), 1);
    assert!(content.raster().rgba.iter().all(|v| *v == 0.0));

    content.update(2, &[Patch { rect, rgba: &[1.0; 16] }]).unwrap();
    assert_eq!(content.version(), 2);
    for (i, pixel) in content.raster().rgba.chunks_exact(4).enumerate() {
        let changed = (3..5).contains(&(i % 6)) && (2..4).contains(&(i / 6));
        assert_eq!(pixel, &[if changed { 1.0 } else { 0.0 }; 4]);
    }
    assert_eq!(rect.expanded(2, viewport.pixels()).unwrap(),
        PixelRect { x: 1, y: 0, width: 5, height: 4 });
}
