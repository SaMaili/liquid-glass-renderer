use liquid_glass_core::interaction::{Input, controller::Controller};

#[test]
fn controller_matches_demo_pointer_resize_and_motion_frames() {
    let mut controller = Controller::new([180.0, 76.0]).unwrap();
    let mut samples = 0;
    let mut idle_frames = 0;
    for (i, line) in include_str!("../../../work/rust-scene/controller-fixture.txt").lines().enumerate() {
        let values: Vec<_> = line.split_whitespace().collect();
        let number = |i: usize| values[i].parse::<f64>().unwrap();
        match values[0] {
            "press" => {
                let input = if values[1] == "touch" { Input::Touch } else { Input::Cursor };
                controller.press(input, [number(2), number(3)]).unwrap();
            }
            "drag" => controller.drag([number(1), number(2)]).unwrap(),
            "release" => controller.release(),
            "resize" => controller.resize([number(1), number(2)]).unwrap(),
            "reduced" => controller.set_reduced_motion(number(1) == 1.0),
            "highlights" => controller.set_reduced_highlighting(number(1) == 1.0),
            "idle" => {
                assert!(controller.sample(number(1)).unwrap().is_none(), "row {i}");
                idle_frames += 1;
            }
            "sample" => {
                let p = controller.sample(number(1)).unwrap().unwrap_or_else(|| panic!("row {i}: missing sample"));
                let [x,y] = p.glow.position;
                let actual = [
                    p.scale[0], p.scale[1], p.translation[0], p.translation[1], p.glow.opacity,
                    p.glow.big_opacity, p.glow.scale, p.glow.diameter, x, y,
                ];
                for (column, value) in actual.into_iter().enumerate() {
                    let expected = number(column + 2);
                    assert!((value - expected).abs() < 1e-10, "row {i}, column {column}: {value} vs {expected}");
                }
                assert_eq!(p.needs_frame, number(12) == 1.0, "row {i}");
                samples += 1;
            }
            unknown => panic!("unknown fixture event {unknown}"),
        }
    }
    assert!(samples > 100);
    assert!(idle_frames > 100);
    assert!(!controller.needs_frame());

    controller.press(Input::Cursor, [5.0, 10.0]).unwrap();
    controller.cancel();
    assert!(!controller.is_pressed());
    let p = controller.sample(20.0).unwrap().unwrap();
    assert_eq!(p.scale, [1.0; 2]);
    assert_eq!(p.translation, [0.0; 2]);
    assert_eq!(p.glow.opacity, 0.0);
    assert!(!p.needs_frame);
    println!("{samples} samples, {idle_frames} idle frame opportunities");
}

#[test]
fn hover_respects_press_pointer_exit_touch_and_cancel() {
    use liquid_glass_core::overlay::Fill;

    let mut controller = Controller::new([180.0, 76.0]).unwrap();
    assert_eq!(controller.fill(), Fill::Idle);
    assert!(controller.sample(0.0).unwrap().is_none());

    controller.set_hovered(true);
    assert_eq!(controller.fill(), Fill::Rollover);
    let hover = controller.sample(0.1).unwrap().unwrap();
    assert_eq!(hover.scale, [1.0; 2]);
    assert_eq!(hover.translation, [0.0; 2]);
    assert_eq!(hover.glow.opacity, 0.0);
    assert_eq!(hover.glow.big_opacity, 0.0);
    assert!(!hover.needs_frame);
    controller.set_hovered(true);
    assert!(controller.sample(0.2).unwrap().is_none());

    controller.set_reduced_motion(true);
    controller.press(Input::Cursor, [90.0, 38.0]).unwrap();
    assert_eq!(controller.fill(), Fill::Pressed);
    controller.sample(0.3).unwrap().unwrap();
    assert!(!controller.needs_frame());
    controller.set_hovered(false);
    assert_eq!(controller.fill(), Fill::Pressed);
    assert!(controller.sample(0.4).unwrap().is_none());
    controller.release();
    assert_eq!(controller.fill(), Fill::Idle);
    controller.sample(0.5).unwrap().unwrap();

    controller.set_hovered(true);
    controller.press(Input::Cursor, [90.0, 38.0]).unwrap();
    controller.release();
    assert_eq!(controller.fill(), Fill::Rollover);
    controller.sample(0.6).unwrap().unwrap();
    controller.set_hovered(false);
    assert_eq!(controller.fill(), Fill::Idle);
    assert!(!controller.sample(0.7).unwrap().unwrap().needs_frame);

    controller.set_hovered(true);
    controller.press(Input::Touch, [90.0, 38.0]).unwrap();
    assert_eq!(controller.fill(), Fill::Pressed);
    controller.set_hovered(true);
    controller.release();
    assert_eq!(controller.fill(), Fill::Idle);
    controller.set_hovered(true);
    controller.cancel();
    assert_eq!(controller.fill(), Fill::Idle);
    assert!(!controller.sample(0.8).unwrap().unwrap().needs_frame);
}

#[test]
fn cursor_hover_composites_the_extracted_palette_at_both_scales() {
    use liquid_glass_core::{
        Renderer,
        frame::Frame,
        overlay::{Appearance, Fill},
        raster::{Scale, Viewport},
        shape::Rectangle,
    };

    let mut controller = Controller::new([28.0, 20.0]).unwrap();
    controller.set_hovered(true);
    let pose = controller.sample(0.1).unwrap().unwrap();
    assert_eq!(controller.fill(), Fill::Rollover);

    for scale in [Scale::One, Scale::Two] {
        let viewport = Viewport::new(48, 40, scale).unwrap();
        let pixels = viewport.pixels();
        let factor = scale.factor();
        let mut renderer = Renderer::new(pixels).unwrap();
        let shape = pose.rectangle(viewport, Rectangle {
            origin: [10.137, 10.193], size: [28.0, 20.0], radii: [8.0; 4], continuous: true,
        }).unwrap();
        let base = [0.5, 0.5, 0.5, 1.0];
        let srgb_to_linear = ((base[0] + 0.055f32) / 1.055).powf(2.4);
        let hover_alpha = 12.0 / 255.0;
        for (appearance, front) in [(Appearance::Light, 0.0), (Appearance::Dark, 1.0)] {
            let mut frame = Frame::new(viewport, vec![base; pixels.pixels()]).unwrap();
            frame.interact(&mut renderer, shape, appearance, controller.fill(), None).unwrap();
            assert_eq!(frame.pixels()[0], base);
            let center = (pixels.width * 20 * factor + 24 * factor) as usize;
            let blended = front * hover_alpha + srgb_to_linear * (1.0 - hover_alpha);
            let expected = 1.055 * blended.powf(1.0 / 2.4) - 0.055;
            for channel in frame.pixels()[center][..3].iter() {
                assert!((channel - expected).abs() < 1e-6, "{appearance:?} at {factor}x");
            }
            assert_eq!(frame.pixels()[center][3], 1.0);
        }
    }
}
