use liquid_glass_core::{
    Renderer,
    frame::{Frame, LightSpot},
    interaction::{Input, controller::Controller},
    overlay::{Appearance, Fill},
    raster::{Scale, Viewport},
    scene::Shape,
    shape::{Rectangle, affine_rectangle},
};

#[test]
fn affine_geometry_matches_executed_original_packing() {
    let mut count = 0;
    for (i, row) in include_str!("../../../work/rust-scene/affine-fixture.txt").lines().enumerate() {
        let parts: Vec<_> = row.split_whitespace().collect();
        let value = |index: usize| parts[index].parse::<f64>().unwrap();
        let bits = |index: usize| parts[index].parse::<u32>().unwrap();
        let rect = Rectangle {
            continuous: parts[0] == "1",
            origin: [value(1), value(2)],
            size: [value(3), value(4)],
            radii: std::array::from_fn(|j| value(j+5)),
        };
        for scale in [Scale::One, Scale::Two] {
            let viewport = Viewport::new(64,48,scale).unwrap();
            let affine = std::array::from_fn(|j| value(j+9));
            let Shape::Geometry { values, transform } = affine_rectangle(viewport, rect, affine).unwrap() else { unreachable!() };
            for (j, component) in values.into_iter().flatten().enumerate() {
                assert_eq!(component.to_bits(), bits(15+j), "packed geometry row {i}, component {j}");
            }
            for (j, component) in transform.into_iter().flatten().enumerate() {
                let expected = f32::from_bits(bits(27+j));
                assert_eq!(component, expected, "transform row {i}, component {j}");
            }
        }
        count += 1;
    }
    assert_eq!(count, 128);
}

#[test]
fn controller_presentation_drives_coverage_and_light_spot_at_both_scales() {
    let rectangle = Rectangle {
        origin: [12.0, 12.0],
        size: [36.0, 24.0],
        radii: [8.0; 4],
        continuous: true,
    };
    for scale in [Scale::One, Scale::Two] {
        let viewport = Viewport::new(64, 48, scale).unwrap();
        let mut renderer = Renderer::new(viewport.pixels()).unwrap();
        let mut input = Controller::new(rectangle.size).unwrap();
        input.press(Input::Cursor, [18.0, 12.0]).unwrap();
        input.drag([5.0, 2.0]).unwrap();
        let mut presentation = input.sample(0.0).unwrap().unwrap();
        for tick in 1..=90 {
            if let Some(next) = input.sample(f64::from(tick)/60.0).unwrap() {
                presentation = next;
            }
        }
        assert!(!input.needs_frame());
        assert!(presentation.scale[0] > 1.0);
        assert!(presentation.glow.opacity > 0.0);
        let mut frame = Frame::new(viewport, vec![[0.35,0.4,0.5,1.0]; viewport.pixels().pixels()]).unwrap();
        let shifted = presentation.rectangle(viewport, rectangle).unwrap();
        frame.interact(&mut renderer, shifted, Appearance::Dark, Fill::Pressed,
            Some(LightSpot { glow: &presentation.glow, origin: rectangle.origin })).unwrap();
        let factor = scale.factor() as usize;
        let feature = (24 * viewport.pixels().width as usize + 30) * factor;
        assert!(frame.pixels()[feature][0] > 0.5);
        assert!((frame.pixels()[0][0] - 0.35).abs() < 1.0e-6);
    }
}
