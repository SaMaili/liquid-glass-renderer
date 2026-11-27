use liquid_glass_core::{
    Renderer,
    frame::{Frame, FrameCache, FrameStage, LightSpot},
    interaction::Glow,
    material::{BackgroundStyle, recipe::{RecipeRole, SdrRecipe}, spec::Dimensions},
    overlay::{Appearance, Fill},
    raster::{Scale, Viewport},
    scene::{Backdrop, Group, MaterialKind},
    shape::{Rectangle, affine_rectangle},
};

fn compare(
    renderer: &mut Renderer, cache: &mut FrameCache, source: &Frame, plan: &[FrameStage<'_>],
) -> (Frame,u64) {
    let before = renderer.stats().draws;
    let cached = cache.render(renderer,source,plan).unwrap();
    let incremental_draws = renderer.stats().draws-before;
    let uncached = FrameCache::new(0).render(renderer,source,plan).unwrap();
    assert_eq!(cached.pixels(),uncached.pixels());
    (cached,incremental_draws)
}

#[test]
fn stable_frames_and_prefixes_are_reused_but_all_visible_inputs_invalidate() {
    let viewport=Viewport::new(80,60,Scale::Two).unwrap();
    let size=viewport.pixels();
    let mut background=vec![[0.25,0.40,0.60,1.0];size.pixels()];
    let source=Frame::new(viewport,background.clone()).unwrap();
    let mut renderer=Renderer::new(size).unwrap();
    let dimensions=Dimensions {minimum:Some(29.0),largest_area_minimum:Some(29.0)};
    let recipe=|tint| SdrRecipe {role:RecipeRole::Regular,appearance:Appearance::Light,dimensions,
        diffusion:0.5,tint,style:BackgroundStyle::default()}.build(viewport).unwrap();
    let plain=recipe(None);
    let colored=recipe(Some([0.2,0.5,0.1,0.85]));
    let fixed=|x| affine_rectangle(viewport,Rectangle {origin:[x,15.193],size:[32.0,28.0],
        radii:[11.0;4],continuous:true},[1.0,0.0,0.0,1.0,0.0,0.0]).unwrap();
    let a=[fixed(9.137)];
    let b=[fixed(35.112)];
    let shifted=[fixed(37.112)];
    let group=|shapes,material,blur_levels| [Group {
        shapes,material,blur_levels,opacity:1.0,smoothness:0.0,shape_tags:&[],tints:&[],
    }];
    let cards=group(&a[..],plain.material.material(MaterialKind::All).unwrap(),plain.blur_levels);
    let button=group(&b[..],plain.material.material(MaterialKind::All).unwrap(),plain.blur_levels);
    let changed_button=group(&shifted[..],plain.material.material(MaterialKind::All).unwrap(),plain.blur_levels);
    let tinted_button=group(&shifted[..],colored.material.material(MaterialKind::All).unwrap(),colored.blur_levels);
    let glass=|groups| FrameStage::Glass {backdrop:Backdrop::Original,groups};
    let light=|glow| LightSpot {glow,origin:[37.112,15.193]};
    let overlay=|appearance,spot| FrameStage::Interaction {shape:shifted[0],appearance,fill:Fill::Pressed,light:Some(spot)};
    let original_plan=[glass(&cards),glass(&button)];
    let mut cache=FrameCache::new(2*1024*1024);
    let (initial,full_draws)=compare(&mut renderer,&mut cache,&source,&original_plan);
    assert!(full_draws>0);
    let before=renderer.stats().draws;
    for _ in 0..32 {
        assert_eq!(cache.render(&mut renderer,&source.clone(),&original_plan).unwrap().pixels(),initial.pixels());
    }
    assert_eq!(renderer.stats().draws,before);
    assert_eq!(cache.stats().hits,32);

    let (moved,moved_draws)=compare(&mut renderer,&mut cache,&source,&[glass(&cards),glass(&changed_button)]);
    assert!(moved_draws<full_draws && moved_draws>0);
    assert_ne!(moved.pixels(),initial.pixels());
    let (tinted,tint_draws)=compare(&mut renderer,&mut cache,&source,&[glass(&cards),glass(&tinted_button)]);
    assert_eq!(tint_draws,moved_draws);
    assert_ne!(tinted.pixels(),moved.pixels());

    let a_glow=Glow {opacity:0.35,big_opacity:0.,scale:1.,diameter:48.,position:[8.,8.]};
    let b_glow=Glow {position:[28.,18.],..a_glow};
    let first=[glass(&cards),glass(&tinted_button),overlay(Appearance::Light,light(&a_glow))];
    let appearance=[glass(&cards),glass(&tinted_button),overlay(Appearance::Dark,light(&a_glow))];
    let drag=[glass(&cards),glass(&tinted_button),overlay(Appearance::Dark,light(&b_glow))];
    let (pressed,_)=compare(&mut renderer,&mut cache,&source,&first);
    let (dark,dark_draws)=compare(&mut renderer,&mut cache,&source,&appearance);
    let (dragged,drag_draws)=compare(&mut renderer,&mut cache,&source,&drag);
    assert_eq!(dark_draws,1);
    assert_eq!(drag_draws,1);
    assert_ne!(pressed.pixels(),dark.pixels());
    assert_ne!(dragged.pixels(),dark.pixels());

    background.fill([0.9,0.1,0.25,1.0]);
    let updated=Frame::new(viewport,background).unwrap();
    let (changed,draws)=compare(&mut renderer,&mut cache,&updated,&drag);
    assert!(draws>full_draws);
    assert_ne!(changed.pixels(),dragged.pixels());

    let mut replacement=Renderer::new(size).unwrap();
    assert_eq!(cache.render(&mut replacement,&updated,&drag).unwrap().pixels(),changed.pixels());
    assert!(replacement.stats().draws>0);
    let before=replacement.stats().draws;
    let broken=[FrameStage::Glass {backdrop:Backdrop::Original,groups:&[
        Group {shapes:&[],material:plain.material.material(MaterialKind::All).unwrap(),smoothness:0.,opacity:1.,blur_levels:1,shape_tags:&[],tints:&[]},
    ]}];
    assert!(cache.render(&mut replacement,&updated,&broken).is_err());
    assert_eq!(cache.render(&mut replacement,&updated,&drag).unwrap().pixels(),changed.pixels());
    assert_eq!(replacement.stats().draws,before);
    assert!(cache.stats().resident_bytes<=2*1024*1024);

    // Budget just large enough for keys and the last image, no stage prefix.
    let small_budget=size.pixels()*16+4096;
    let mut small=FrameCache::new(small_budget);
    compare(&mut renderer,&mut small,&source,&original_plan);
    assert_eq!(compare(&mut renderer,&mut small,&source,&original_plan).1,0);
    assert_eq!(compare(&mut renderer,&mut small,&source,&[glass(&cards),glass(&changed_button)]).1,full_draws);
    assert!(small.stats().resident_bytes<=small_budget);
}

#[test]
fn resizing_and_removing_groups_preserves_pixels_and_bounds_resident_storage() {
    let cache_budget = 1024 * 1024;
    let mut cache = FrameCache::new(cache_budget);
    let mut resident_after_warmup = [None; 2];
    let mut previous_output: Option<Frame> = None;

    for cycle in 0..16 {
        let index = cycle % 2;
        let scale = [Scale::One, Scale::Two][index];
        let viewport = Viewport::new(56, 40, scale).unwrap();
        let size = viewport.pixels();
        let texture_budget = size.pixels() * 16 * 27 / 8;
        let mut renderer = Renderer::with_budget(size, texture_budget).unwrap();
        let backdrop = Frame::new(
            viewport,
            (0..size.pixels())
                .map(|i| [0.1 + (i % 23) as f32 / 100.0, 0.45, 0.28, 1.0])
                .collect(),
        )
        .unwrap();
        let recipe = SdrRecipe {
            role: RecipeRole::Regular,
            appearance: Appearance::Light,
            dimensions: Dimensions { minimum: Some(24.0), largest_area_minimum: Some(24.0) },
            diffusion: 0.5,
            tint: None,
            style: BackgroundStyle::default(),
        }
        .build(viewport)
        .unwrap();
        let group_at = |x| {
            affine_rectangle(
                viewport,
                Rectangle {
                    origin: [x, 8.0],
                    size: [26.0, 24.0],
                    radii: [8.0; 4],
                    continuous: true,
                },
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            )
            .unwrap()
        };
        let left = [group_at(7.0)];
        let right = [group_at(22.0)];
        let group = |shapes| Group {
            shapes,
            shape_tags: &[],
            tints: &[],
            material: recipe.material.material(MaterialKind::All).unwrap(),
            blur_levels: recipe.blur_levels,
            opacity: 1.0,
            smoothness: 0.0,
        };
        let groups = [group(&left), group(&right)];
        let separate = [
            FrameStage::Glass { backdrop: Backdrop::Original, groups: &groups[..1] },
            FrameStage::Glass { backdrop: Backdrop::Original, groups: &groups[1..] },
        ];
        let (full, draws) = compare(&mut renderer, &mut cache, &backdrop, &separate);
        assert!(draws > 0);
        let warm = renderer.stats();

        for _ in 0..4 {
            let (one, _) = compare(&mut renderer, &mut cache, &backdrop, &separate[..1]);
            assert_ne!(one.pixels(), full.pixels());
            let (empty, _) = compare(&mut renderer, &mut cache, &backdrop, &[]);
            assert_eq!(empty.pixels(), backdrop.pixels());
            let (restored, _) = compare(&mut renderer, &mut cache, &backdrop, &separate);
            assert_eq!(restored.pixels(), full.pixels());

            let before = renderer.stats().draws;
            let hit = cache.render(&mut renderer, &backdrop, &separate).unwrap();
            assert_eq!(hit.pixels(), full.pixels());
            assert_eq!(renderer.stats().draws, before);
            assert!(renderer.stats().resident_texture_bytes <= texture_budget);
            assert!(cache.stats().resident_bytes <= cache_budget);
        }

        let stats = renderer.stats();
        assert!(stats.texture_evictions > warm.texture_evictions);
        assert_eq!(stats.buffer_allocations, warm.buffer_allocations);
        assert_eq!(stats.program_compilations, warm.program_compilations);
        let resident = (
            stats.resident_texture_bytes,
            stats.resident_buffer_bytes,
            cache.stats().resident_bytes,
        );
        assert_eq!(*resident_after_warmup[index].get_or_insert(resident), resident);

        // Keep one caller-owned frame across renderer destruction and replacement.
        if let Some(previous) = previous_output.replace(full) {
            assert_ne!(previous.pixels().len(), backdrop.pixels().len());
        }
    }
    assert_eq!(cache.stats().hits, 16 * 4);
}
