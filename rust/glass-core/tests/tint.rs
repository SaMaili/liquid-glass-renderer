use liquid_glass_core::{
    Raster, Renderer,
    frame::{Frame, FrameCache, FrameStage},
    material::{BackgroundStyle, recipe::{PreparedSdrMaterial, RecipeRole, SdrRecipe}, spec::Dimensions},
    overlay::Appearance,
    raster::{Content, Patch, PixelRect, Scale, Viewport},
    scene::{self, Backdrop, Group, LiveScene, MaterialKind, Shape, ShapeFilter, Tint, TintRamp},
};

fn recipe(viewport: Viewport) -> PreparedSdrMaterial {
    SdrRecipe {
        role: RecipeRole::Regular,
        appearance: Appearance::Light,
        dimensions: Dimensions { minimum: Some(28.0), largest_area_minimum: Some(28.0) },
        diffusion: 0.4,
        tint: None,
        style: BackgroundStyle::default(),
    }.build(viewport).unwrap()
}

fn rectangle(viewport: Viewport, left: f32, right: f32) -> Vec<f32> {
    let size = viewport.pixels();
    (0..size.height).flat_map(|y| (0..size.width).flat_map(move |x| {
        let [x, y] = viewport.pixel_center(x, y).unwrap().map(|v| v as f32);
        let (distance, nx, ny) = [(left-x, -1.0, 0.0), (x-right, 1.0, 0.0),
            (5.25-y, 0.0, -1.0), (y-30.25, 0.0, 1.0)].into_iter()
            .max_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        [distance, nx, ny, 1.0]
    })).collect()
}

fn tint(filter: ShapeFilter, ramp: TintRamp) -> Tint {
    Tint {
        shape_filter: filter, ramp, opacity: 1.0,
        matrix: [[0.0, 0.0, 0.0, 0.0, 0.9], [0.0; 5], [0.0; 5], [0.0, 0.0, 0.0, 0.0, 1.0]],
        max_color_component: 1.0, preserves_hue: false,
    }
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.00005, "{actual} != {expected}");
}

#[test]
fn shape_tags_limit_tint_and_soft_fade_without_tinting_the_shadow() {
    for scale in [Scale::One, Scale::Two] {
        let viewport = Viewport::new(81, 39, scale).unwrap();
        let size = viewport.pixels();
        let mut renderer = Renderer::new(size).unwrap();
        let backing = [0.18, 0.35, 0.55, 1.0];
        let content = Content::new(viewport, 0, backing.repeat(size.pixels())).unwrap();
        let fields = [
            rectangle(viewport, 8.0, 22.25),
            rectangle(viewport, 15.0, 58.25),
            rectangle(viewport, 65.0, 75.0),
        ];
        let shapes = fields.each_ref().map(|rgba| Shape::Field(Raster { size, rgba }));
        let prepared = recipe(viewport);
        let draw = |renderer: &mut Renderer, tints: &[Tint], opacity| scene::render(
            renderer, &content, Backdrop::Original, &[Group {
                shapes: &shapes, shape_tags: &[3, 2, 5], tints, opacity, smoothness: 0.0,
                blur_levels: prepared.blur_levels, material: prepared.material.material(MaterialKind::All).unwrap(),
            }],
        ).unwrap();
        let plain = draw(&mut renderer, &[], 1.0);
        let filter = ShapeFilter { mask: 1, value: 1 };
        let sharp = tint(filter, TintRamp::SHARP);
        let soft = tint(filter, TintRamp::SOFT);
        let sharp_pixels = draw(&mut renderer, &[sharp], 1.0);
        let soft_pixels = draw(&mut renderer, &[soft], 1.0);
        let far_ramp = Tint { ramp: TintRamp::Soft { opaque_through: 45.0, transparent_at: 50.0 }, ..soft };
        let full_ramp = draw(&mut renderer, &[far_ramp], 1.0);
        let mut copy_red_to_green = tint(filter, TintRamp::SHARP);
        copy_red_to_green.matrix[0] = [0.0; 5];
        copy_red_to_green.matrix[1] = [1.0, 0.0, 0.0, 0.0, 0.0];
        let ordered = draw(&mut renderer, &[sharp, copy_red_to_green], 1.0);
        let reversed = draw(&mut renderer, &[copy_red_to_green, sharp], 1.0);
        let faded = draw(&mut renderer, &[sharp, copy_red_to_green], 0.4);
        let missing = draw(&mut renderer, &[tint(ShapeFilter::exact(1), TintRamp::SHARP)], 1.0);
        assert_eq!(missing, plain);

        let factor = scale.factor();
        let index = |x| ((17 * factor * size.width + x) * 4) as usize;
        let overlap = index(18 * factor);
        assert_ne!(&plain[overlap..overlap+4], &backing);
        close(sharp_pixels[overlap], 0.9);
        close(sharp_pixels[index(70 * factor)], 0.9);
        close(ordered[overlap+1], 0.9);
        close(reversed[overlap+1], 0.0);
        close(faded[overlap+1], backing[1] * 0.6 + 0.9 * 0.4);

        let spill = index(24 * factor);
        assert_eq!(sharp_pixels[spill], plain[spill]);
        // Evaluate the recorded cubic at distances 2.25 pt (1×) and 2 pt (2×).
        let weight = match scale { Scale::One => 0.9397501, Scale::Two => 0.95360094 };
        close(soft_pixels[spill], 0.9 * weight + plain[spill] * (1.0 - weight));
        let outside_ramp = index(36 * factor);
        assert_eq!(&soft_pixels[outside_ramp..outside_ramp+4], &plain[outside_ramp..outside_ramp+4]);

        let edge = index(58 * factor);
        let coverage = match scale { Scale::One => 0.25, Scale::Two => 0.5 };
        close(full_ramp[edge], 0.9 * coverage + plain[edge] * (1.0 - coverage));
        let outside = index(59 * factor);
        assert_eq!(&full_ramp[outside..outside+4], &plain[outside..outside+4]);
    }
}

#[test]
fn tinted_groups_track_backdrop_updates_and_cache_tag_and_matrix_changes() {
    let viewport = Viewport::new(161, 131, Scale::One).unwrap();
    let size = viewport.pixels();
    let mut renderer = Renderer::new(size).unwrap();
    let backdrop = [0.2, 0.25, 0.3, 0.8];
    let content = Content::new(viewport, 1, backdrop.repeat(size.pixels())).unwrap();
    let fields = [rectangle(viewport, 85.0, 105.0), rectangle(viewport, 100.0, 138.0)];
    let shapes = fields.each_ref().map(|rgba| Shape::Field(Raster { size, rgba }));
    let prepared = recipe(viewport);
    let mut effect = tint(ShapeFilter::exact(1), TintRamp::SOFT);
    effect.matrix[0] = [0.65, 0.2, 0.0, 0.0, 0.12];
    effect.matrix[3] = [0.0, 0.0, 0.0, 0.5, 0.1];
    effect.opacity = 0.7;
    let effects = [effect];
    let group = |shapes, tags, tints| Group {
        shapes, shape_tags: tags, tints, opacity: 0.8, smoothness: 2.0,
        blur_levels: 1, material: prepared.material.material(MaterialKind::All).unwrap(),
    };
    let groups = [group(&shapes[..], &[1, 2], &effects), group(&shapes[1..], &[], &[])];
    let mut live = LiveScene::new(&mut renderer, content.clone(), Backdrop::Current, &groups, 2_000_000).unwrap();
    let mut updated = content.clone();
    let rect = PixelRect { x: 104, y: 17, width: 3, height: 5 };
    let rgba = [0.59, 0.12, 0.07, 0.75].repeat((rect.width * rect.height) as usize);
    let patches = [Patch { rect, rgba: &rgba }];
    let damage = live.update(&mut renderer, 2, &patches).unwrap();
    assert!(damage.x > 0, "{damage:?}");
    updated.update(2, &patches).unwrap();
    let full = scene::render(&mut renderer, &updated, Backdrop::Current, &groups).unwrap();
    for (&actual, expected) in live.pixels().iter().zip(full) { close(actual, expected); }

    let source = Frame::new(viewport, vec![backdrop; size.pixels()]).unwrap();
    let mut cache = FrameCache::new(2_000_000);
    let draw = |cache: &mut FrameCache, renderer: &mut Renderer, groups| cache.render(
        renderer, &source, &[FrameStage::Glass { backdrop: Backdrop::Current, groups }],
    ).unwrap();
    let initial = draw(&mut cache, &mut renderer, &groups);
    assert_eq!(draw(&mut cache, &mut renderer, &groups).pixels(), initial.pixels());
    assert_eq!(cache.stats().hits, 1);
    let swapped = [group(&shapes[..], &[2, 1], &effects), group(&shapes[1..], &[], &[])];
    let swapped_frame = draw(&mut cache, &mut renderer, &swapped);
    assert_ne!(swapped_frame.pixels(), initial.pixels());
    effect.matrix[0][4] += 0.2;
    let updated_effects = [effect];
    let recolored = [group(&shapes[..], &[2, 1], &updated_effects), group(&shapes[1..], &[], &[])];
    assert_ne!(draw(&mut cache, &mut renderer, &recolored).pixels(), swapped_frame.pixels());
    assert_eq!(cache.stats().misses, 3);
}
