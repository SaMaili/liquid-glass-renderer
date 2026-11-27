use liquid_glass_core::{
    Renderer,
    frame::Frame,
    matching::{GlassContainer, GlassEntry, evaluate_geometry_match},
    material::recipe::RecipeRole,
    overlay::Appearance,
    raster::{Scale, Viewport},
    shape::Rectangle,
};

fn rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Rectangle {
    Rectangle {
        origin: [x, y],
        size: [w, h],
        radii: [r; 4],
        continuous: true,
    }
}

fn solid_frame(viewport: Viewport, rgba: [f32; 4]) -> Frame {
    let count = viewport.pixels().pixels();
    Frame::new(viewport, vec![rgba; count]).unwrap()
}

#[test]
fn directed_factor3_frinta_distance_and_stepping_stone_match_oracle() {
    let m1 = evaluate_geometry_match(
        rect(0.0, 0.0, 20.0, 20.0, 4.0),
        2,
        rect(30.3, 14.4, 20.0, 20.0, 4.0),
        0.5,
    )
    .unwrap();
    assert!((m1.distance - 10.5).abs() < 1.0e-12);
    let off1 = m1.min_offset.unwrap();
    assert!((off1[0] - (40.3 - 10.0)).abs() < 1.0e-15 && off1[1] == 0.0);

    let m_diag = evaluate_geometry_match(
        rect(0.0, 0.0, 20.0, 20.0, 4.0),
        2,
        rect(30.3, 28.4, 20.0, 20.0, 4.0),
        0.5,
    )
    .unwrap();
    assert!((m_diag.distance - 13.509256086106296).abs() < 1.0e-12);

    let bar30 = rect(0.0, 0.0, 30.0, 10.0, 5.0);
    let bar29 = rect(0.0, 0.0, 29.0, 10.0, 5.0);
    let btn10 = rect(45.0, 0.0, 10.0, 10.0, 5.0);
    assert!(evaluate_geometry_match(btn10, 1, bar30, 1.0).is_some());
    assert!(evaluate_geometry_match(bar30, 3, btn10, 1.0).is_none());
    assert!(evaluate_geometry_match(bar29, 3, btn10, 1.0).is_some());

    let vp = Viewport::new(128, 64, Scale::One).unwrap();
    let src = solid_frame(vp, [0.1, 0.1, 0.1, 1.0]);

    let mut alone = GlassContainer::new();
    alone.set_max_match_distance(9.0).unwrap();
    let e_base = GlassEntry::new(1, rect(0.0, 10.0, 10.0, 10.0, 5.0));
    let e_far = GlassEntry::new(2, rect(20.0, 10.0, 10.0, 10.0, 5.0));
    alone.submit(&[e_base], 0.0).unwrap();
    alone.submit(&[e_base, e_far], 0.016).unwrap();
    alone.advance(0.10).unwrap();
    let groups_alone = alone.resolve(&src).unwrap();
    let far_entry = groups_alone.iter().flat_map(|g| &g.entries).find(|e| e.id == 2).unwrap();
    assert_eq!(far_entry.matched_partner_id, None);

    let mut bridged = GlassContainer::new();
    bridged.set_max_match_distance(9.0).unwrap();
    let e_mid = GlassEntry::new(3, rect(5.0, 10.0, 10.0, 10.0, 5.0));
    bridged.submit(&[e_base], 0.0).unwrap();
    bridged.submit(&[e_base, e_mid, e_far], 0.016).unwrap();
    bridged.advance(0.10).unwrap();
    let groups_bridged = bridged.resolve(&src).unwrap();
    let bridged_entry = groups_bridged.iter().flat_map(|g| &g.entries).find(|e| e.id == 2).unwrap();
    assert_eq!(bridged_entry.matched_partner_id, Some(1));
}

#[test]
fn automatic_spatial_mitosis_emerges_from_partner_and_retracts_on_removal() {
    let vp = Viewport::new(360, 160, Scale::One).unwrap();
    let src = solid_frame(vp, [0.15, 0.18, 0.24, 1.0]);
    let mut container = GlassContainer::new();

    let mut bar = GlassEntry::new(1, rect(60.0, 56.0, 120.0, 48.0, 24.0));
    bar.smoothness = 26.0;

    let mut droplet = GlassEntry::new(2, rect(210.0, 56.0, 48.0, 48.0, 24.0));
    droplet.smoothness = 26.0;

    container.submit(&[bar], 0.0).unwrap();
    container.submit(&[bar, droplet], 1.0 / 60.0).unwrap();

    // Immediately after spawn, droplet starts tucked inside `bar` (`origin[0] < 180.0`)
    // so it emerges organically through `bar`'s right wall via `sdf-union.frag`!
    container.advance(2.0 / 60.0).unwrap();
    let early = container.resolve(&src).unwrap();
    assert_eq!(early.len(), 1);
    let early_drop = early[0].entries.iter().find(|e| e.id == 2).unwrap();
    assert_eq!(early_drop.matched_partner_id, Some(1));
    assert!(early_drop.rect.origin[0] < 180.0, "matched droplet must emerge from inside partner");

    // Settle at target `(210.0, 56.0)` (`gap = 30 > smoothness = 26`, cleanly pinched off!)
    let mut t = 2.0 / 60.0;
    while t < 1.6 {
        t += 1.0 / 60.0;
        container.advance(t).unwrap();
    }
    let settled = container.resolve(&src).unwrap();
    let settled_drop = settled[0].entries.iter().find(|e| e.id == 2).unwrap();
    assert!((settled_drop.rect.origin[0] - 210.0).abs() < 0.5);

    // Remove droplet: it retracts back toward `bar` before pruning!
    container.submit(&[bar], t + 1.0 / 60.0).unwrap();
    t += 6.0 / 60.0;
    container.advance(t).unwrap();
    let retracting = container.resolve(&src).unwrap();
    let ret_drop = retracting[0].entries.iter().find(|e| e.id == 2).unwrap();
    assert!(ret_drop.rect.origin[0] < 195.0, "departing droplet must retract toward partner");
}

#[test]
fn unmatched_materialize_uses_anisotropic_16pt_capped_vapor_expansion() {
    let vp = Viewport::new(400, 200, Scale::One).unwrap();
    let src = solid_frame(vp, [0.1, 0.1, 0.1, 1.0]);
    let mut container = GlassContainer::new();

    let anchor = GlassEntry::new(1, rect(10.0, 10.0, 20.0, 20.0, 6.0));
    container.submit(&[anchor], 0.0).unwrap();

    let wide = GlassEntry::new(99, rect(150.0, 80.0, 200.0, 50.0, 16.0));
    container.submit(&[anchor, wide], 1.0 / 60.0).unwrap();
    container.advance(2.0 / 60.0).unwrap();

    let groups = container.resolve(&src).unwrap();
    let resolved_wide = groups.iter().flat_map(|g| &g.entries).find(|e| e.id == 99).unwrap();
    assert!(resolved_wide.rect.size[0] > 205.0 && resolved_wide.rect.size[0] <= 216.01);
    assert!(resolved_wide.rect.size[1] > 52.0 && resolved_wide.rect.size[1] <= 60.01);
}

#[test]
fn conditional_z_sort_preserves_painters_order_and_renders_on_gpu() {
    let vp = Viewport::new(192, 128, Scale::One).unwrap();
    let mut pixels = vec![[0.04, 0.04, 0.05, 1.0]; vp.pixels().pixels()];
    for y in 0..64 {
        for x in 0..192 {
            pixels[y * 192 + x] = [0.98, 0.98, 0.99, 1.0];
        }
    }
    let src = Frame::new(vp, pixels).unwrap();

    let mut g0 = GlassEntry::new(1, rect(24.0, 72.0, 56.0, 40.0, 16.0));
    g0.z_index = 0;
    g0.smoothness = 20.0;
    g0.adaptive = true;
    g0.appearance = Appearance::Light;

    let mut g1 = GlassEntry::new(2, rect(52.0, 44.0, 56.0, 48.0, 20.0));
    g1.z_index = 1;
    g1.role = RecipeRole::Clear;
    g1.smoothness = 20.0;

    let mut g2 = GlassEntry::new(3, rect(80.0, 14.0, 56.0, 40.0, 16.0));
    g2.z_index = 2;
    g2.smoothness = 20.0;
    g2.adaptive = true;
    g2.appearance = Appearance::Light;

    let mut container = GlassContainer::new();
    container.submit(&[g2, g0, g1], 0.0).unwrap();

    // First resolve initializes per-entry adapted appearances (`Dark` at y=72, `Light` at y=14)
    let groups = container.resolve(&src).unwrap();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].entries[0].id, 1);
    assert_eq!(groups[0].appearance, Appearance::Dark);
    assert_eq!(groups[1].entries[0].id, 2);
    assert_eq!(groups[1].role, RecipeRole::Clear);
    assert_eq!(groups[2].entries[0].id, 3);
    assert_eq!(groups[2].appearance, Appearance::Light);

    let mut renderer = Renderer::new(vp.pixels()).unwrap();
    let out = container.render(&mut renderer, &src, 1.0 / 60.0).unwrap();
    assert_eq!(out.pixels().len(), vp.pixels().pixels());
}

#[test]
fn adaptive_appearance_transitions_smoothly_via_default_fluid_spring() {
    let vp = Viewport::new(128, 64, Scale::One).unwrap();
    let bright = solid_frame(vp, [0.96, 0.96, 0.98, 1.0]);
    let dark = solid_frame(vp, [0.02, 0.02, 0.03, 1.0]);

    let mut pill = GlassEntry::new(1, rect(16.0, 12.0, 88.0, 40.0, 20.0));
    pill.adaptive = true;
    pill.appearance = Appearance::Light;

    let mut container = GlassContainer::new();
    container.submit(&[pill], 0.0).unwrap();

    // Initial observation on bright frame snaps cleanly to Light (`dark_mix == 0.0`)
    let g0 = container.resolve(&bright).unwrap();
    assert_eq!(g0[0].entries[0].dark_mix, 0.0);

    // Switch backdrop to dark: `target_dark_mix` becomes 1.0, and stepping `0.10s` into
    // `Animation.default` (`FluidSpring(0.50, 1.00)`) yields a smooth intermediate mix!
    let _ = container.resolve(&dark).unwrap();
    let mut t = 0.0;
    while t < 0.10 {
        t += 1.0 / 60.0;
        container.advance(t).unwrap();
    }
    let g_mid = container.resolve(&dark).unwrap();
    let mix = g_mid[0].entries[0].dark_mix;
    assert!(
        mix > 0.15 && mix < 0.85,
        "Light->Dark must interpolate smoothly via FluidSpring(0.50, 1.00), got {mix}"
    );

    // After full settle (`t = 1.5s`), reaches pure Dark (`1.0`)
    while t < 1.5 {
        t += 1.0 / 60.0;
        container.advance(t).unwrap();
    }
    let g_end = container.resolve(&dark).unwrap();
    assert!((g_end[0].entries[0].dark_mix - 1.0).abs() < 0.01);
}
