use liquid_glass_core::{
    frame::Frame,
    material::{
        BackgroundStyle,
        adaptive::{BackdropState, Profile, Selection, measure},
        recipe::{RecipeRole, SdrRecipe},
        spec::Dimensions,
    },
    overlay::Appearance,
    raster::{PixelRect, Scale, Viewport},
};

fn appearance(value: &str) -> Option<Appearance> {
    match value {
        "-" => None,
        "light" => Some(Appearance::Light),
        "dark" => Some(Appearance::Dark),
        _ => panic!("unexpected fixture appearance"),
    }
}

fn optional<T: std::str::FromStr>(value: &str) -> Option<T> {
    (value != "-").then(|| value.parse().ok().expect("invalid fixture value"))
}

#[test]
fn backdrop_state_matches_executed_v2_gates_and_luminance_updates() {
    let viewport = Viewport::new(3, 2, Scale::Two).unwrap();
    let bounds = PixelRect { x: 2, y: 1, width: 2, height: 2 };
    let mut state = BackdropState::default();
    let mut previous_sequence = None;
    let mut active_frames = 0;
    let mut inactive_frames = 0;
    for line in include_str!("fixtures/backdrop-appearance.txt").lines() {
        let cells: Vec<_> = line.split_ascii_whitespace().collect();
        let [
            sequence, role, environment, enabled, reduce, background, added, removed, fixed,
            initial, explicit, distance, surface, red, green, blue, alpha, observe, measured,
            active, selected, luminance,
        ] = cells.as_slice() else {
            panic!("invalid backdrop fixture row");
        };
        if previous_sequence.replace(*sequence) != Some(sequence) {
            state = BackdropState::default();
        }
        let role = match *role {
            "0" => RecipeRole::Regular,
            "2" => RecipeRole::Clear,
            "3" => RecipeRole::Dock,
            _ => panic!("unknown fixture role"),
        };
        let recipe = SdrRecipe {
            role,
            appearance: appearance(environment).unwrap(),
            dimensions: Dimensions { minimum: Some(38.0), largest_area_minimum: optional(distance) },
            diffusion: 0.5,
            tint: None,
            style: BackgroundStyle::default(),
        };
        let profile = Profile {
            adaptive_v2_enabled: *enabled == "1",
            background_content_word: background.parse().unwrap(),
            reduce_transparency: *reduce == "1",
            options_added: added.parse().unwrap(),
            options_removed: removed.parse().unwrap(),
            fixed_luminance: optional(fixed),
            initial_luminance: optional(initial),
            explicit_appearance: appearance(explicit),
            ..Profile::default()
        };
        let color = [red, green, blue, alpha].map(|value| value.parse().unwrap());
        let size = viewport.pixels();
        let mut pixels = vec![[0.95, 0.0, 0.0, 1.0]; size.pixels()];
        for y in bounds.y..bounds.y + bounds.height {
            for x in bounds.x..bounds.x + bounds.width {
                pixels[(y * size.width + x) as usize] = color;
            }
        }
        let frame = Frame::new(viewport, pixels).unwrap();
        let observation = (*observe == "1").then_some(bounds);
        if observation.is_some() {
            assert_eq!(measure(&frame, bounds).unwrap(), optional(measured), "{line}");
        }
        let result = state.update(&recipe, profile, surface.parse().unwrap(), &frame, observation).unwrap();
        assert_eq!(result, Selection {
            active: *active == "1",
            appearance: appearance(selected).unwrap(),
            luminance: optional(luminance),
        }, "{line}");
        if result.active {
            active_frames += 1;
        } else {
            inactive_frames += 1;
        }
    }
    assert_eq!(active_frames + inactive_frames, 480);
    assert!(active_frames > 50 && inactive_frames > 50);
}

#[test]
fn gated_observation_is_skipped_and_failed_measurement_preserves_state() {
    let frame = Frame::new(Viewport::new(3, 2, Scale::One).unwrap(), vec![[0.0, 0.0, 0.0, 1.0]; 6])
        .unwrap();
    let recipe = SdrRecipe {
        role: RecipeRole::Regular,
        appearance: Appearance::Light,
        dimensions: Dimensions { minimum: Some(38.0), largest_area_minimum: Some(38.0) },
        diffusion: 0.5,
        tint: None,
        style: BackgroundStyle::default(),
    };
    let outside = PixelRect { x: 3, y: 0, width: 1, height: 1 };
    let inside = PixelRect { x: 1, y: 0, width: 1, height: 1 };
    let mut state = BackdropState::default();
    assert!(!state.update(&recipe, Profile::default(), 0, &frame, Some(outside)).unwrap().active);

    let enabled = Profile { adaptive_v2_enabled: true, options_added: 0x10000, ..Profile::default() };
    let initial = state.update(&recipe, enabled, 0, &frame, Some(inside)).unwrap();
    assert_eq!(initial.luminance, Some(0.0));
    assert_eq!(initial.appearance, Appearance::Dark);
    assert!(state.update(&recipe, enabled, 0, &frame, Some(outside)).is_err());
    assert_eq!(state.update(&recipe, enabled, 0, &frame, None).unwrap(), initial);
}

#[test]
fn selected_luminance_changes_the_rendered_material_even_with_the_same_appearance() {
    use liquid_glass_core::{
        Renderer,
        scene::{Backdrop, Group, MaterialKind},
        shape::{Rectangle, affine_rectangle},
    };

    for scale in [Scale::One, Scale::Two] {
        let viewport = Viewport::new(80, 60, scale).unwrap();
        let size = viewport.pixels();
        let source = Frame::new(viewport, vec![[0.18, 0.23, 0.30, 1.0]; size.pixels()]).unwrap();
        let bounds = PixelRect { x: 0, y: 0, width: size.width, height: size.height };
        let recipe = || SdrRecipe {
            role: RecipeRole::Regular,
            appearance: Appearance::Dark,
            dimensions: Dimensions { minimum: Some(38.0), largest_area_minimum: Some(38.0) },
            diffusion: 0.5,
            tint: None,
            style: BackgroundStyle::default(),
        };
        let mut state = BackdropState::default();
        let (inactive, retail) = state.prepare(recipe(), Profile::default(), 0, &source, Some(bounds)).unwrap();
        let profile = Profile { adaptive_v2_enabled: true, ..Profile::default() };
        let (active, adapted) = state.prepare(recipe(), profile, 0, &source, Some(bounds)).unwrap();
        assert!(!inactive.active && active.active);
        assert_eq!(inactive.appearance, active.appearance);
        assert_eq!(active.luminance, measure(&source, bounds).unwrap());
        assert!(active.luminance.is_some());

        let origin = match scale {
            Scale::One => [16.5, 11.5],
            Scale::Two => [16.25, 11.75],
        };
        let shapes = [affine_rectangle(
            viewport,
            Rectangle { origin, size: [48.0, 38.0], radii: [18.0; 4], continuous: true },
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        ).unwrap()];
        let mut renderer = Renderer::new(size).unwrap();
        let mut plain_frame = source.clone();
        let mut adapted_frame = source.clone();
        for (frame, prepared) in [(&mut plain_frame, retail), (&mut adapted_frame, adapted)] {
            frame.render_groups(&mut renderer, Backdrop::Original, &[Group {
                shapes: &shapes,
                shape_tags: &[],
                tints: &[],
                material: prepared.material.material(MaterialKind::All).unwrap(),
                smoothness: 0.0,
                opacity: 1.0,
                blur_levels: prepared.blur_levels,
            }]).unwrap();
        }
        assert_eq!(plain_frame.pixels()[0], adapted_frame.pixels()[0]);
        assert_ne!(plain_frame.pixels(), adapted_frame.pixels());
    }
}
