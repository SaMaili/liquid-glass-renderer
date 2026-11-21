use liquid_glass_core::{Size, scene::blur_level_count};

#[test]
fn recovered_blur_level_decisions() {
    // Decision boundaries from shaders/check_blur_levels.py.
    let large = Size::new(1024, 1024).unwrap();
    for (s1, count) in [(0.0, 1), (0.01, 2), (1.25, 2), (1.25001, 3)] {
        assert_eq!(blur_level_count(large, s1).unwrap(), count);
    }
    assert_eq!(blur_level_count(Size::new(3, 8).unwrap(), 100_000.0).unwrap(), 4);
    assert_eq!(blur_level_count(Size::new(1, 1).unwrap(), 100.0).unwrap(), 1);
    assert!(blur_level_count(large, -1.0).is_err());
    assert!(blur_level_count(large, f32::MAX).is_err());
}
