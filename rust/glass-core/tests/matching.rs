use liquid_glass_core::{
    Renderer, Size,
    frame::Frame,
    matching::{GlassContainer, GlassEntry},
    material::recipe::RecipeRole,
    overlay::Appearance,
    raster::{Scale, Viewport},
    shape::Rectangle,
};

fn solid_frame(width: u32, height: u32, rgba: [f32; 4]) -> Frame {
    let vp = Viewport::new(width, height, Scale::One).unwrap();
    let pixels = vec![rgba; vp.pixels().pixels()];
    Frame::new(vp, pixels).unwrap()
}

fn rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Rectangle {
    Rectangle { origin: [x, y], size: [w, h], radii: [r; 4], continuous: true }
}

#[test]
fn cross_id_match_transfers_spring_state_smoothly() {
    let frame = solid_frame(320, 200, [0.2, 0.25, 0.3, 1.0]);
    let mut container = GlassContainer::new();

    // Initial compact pill (id = 1, match_id = 42)
    let mut a = GlassEntry::new(1, rect(20.0, 20.0, 60.0, 56.0, 28.0));
    a.match_id = Some(42);
    container.submit(&[a], 0.0).unwrap();

    // Replace id = 1 with expanded panel id = 2 sharing match_id = 42
    let mut b = GlassEntry::new(2, rect(20.0, 20.0, 260.0, 140.0, 20.0));
    b.match_id = Some(42);
    container.submit(&[b], 0.0).unwrap();

    // Mid-transition (t = 0.08s): entry 2 must be partway between 60pt and 260pt!
    assert!(container.advance(0.08).unwrap());
    let groups = container.resolve(&frame).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entries.len(), 1);
    let mid_w = groups[0].entries[0].rect.size[0];
    assert!(mid_w > 65.0 && mid_w < 245.0, "expected mid-morph width, got {mid_w}");

    // Settled (step at 60 Hz for 1.5s): reaches target width 260pt
    for step in 6..=90 {
        container.advance(step as f64 / 60.0).unwrap();
    }
    let settled = container.resolve(&frame).unwrap();
    assert!((settled[0].entries[0].rect.size[0] - 260.0).abs() < 1.0);
}

#[test]
fn automatic_material_grouping_merges_compatible_and_splits_incompatible() {
    let frame = solid_frame(400, 200, [0.15, 0.15, 0.2, 1.0]);
    let mut container = GlassContainer::new();

    // Two nearby Regular/Dark entries with smoothness -> should merge into 1 group
    let mut left = GlassEntry::new(1, rect(20.0, 40.0, 80.0, 56.0, 28.0));
    left.smoothness = 14.0;
    let mut right = GlassEntry::new(2, rect(110.0, 40.0, 54.0, 54.0, 27.0));
    right.smoothness = 14.0;

    // A Clear entry right next to them -> incompatible role, must stay in its own group
    let mut clear = GlassEntry::new(3, rect(170.0, 40.0, 54.0, 54.0, 27.0));
    clear.role = RecipeRole::Clear;
    clear.smoothness = 14.0;

    // A distant Regular/Dark entry -> outside smoothness*2.5 reach, must stay separate
    let mut far = GlassEntry::new(4, rect(320.0, 40.0, 54.0, 54.0, 27.0));
    far.smoothness = 14.0;

    container.submit(&[left, right, clear, far], 0.0).unwrap();
    let groups = container.resolve(&frame).unwrap();
    assert_eq!(groups.len(), 3, "expected [left+right], [clear], [far]");
    assert_eq!(groups[0].entries.len(), 2);
    assert_eq!(groups[1].entries.len(), 1);
    assert_eq!(groups[1].role, RecipeRole::Clear);
    assert_eq!(groups[2].entries.len(), 1);
}

#[test]
fn automatic_backdrop_adaptation_tracks_bright_and_dark_frames() {
    let bright = solid_frame(200, 200, [0.99, 0.99, 0.99, 1.0]);
    let dark = solid_frame(200, 200, [0.02, 0.02, 0.03, 1.0]);
    let mut container = GlassContainer::new();

    let mut pill = GlassEntry::new(10, rect(40.0, 60.0, 120.0, 48.0, 24.0));
    pill.adaptive = true;
    pill.appearance = Appearance::Dark;
    container.submit(&[pill], 0.0).unwrap();

    // Over bright backdrop -> adapts automatically to Light
    let g_bright = container.resolve(&bright).unwrap();
    assert_eq!(g_bright[0].appearance, Appearance::Light);

    // Over dark backdrop -> adapts automatically back to Dark
    let g_dark = container.resolve(&dark).unwrap();
    assert_eq!(g_dark[0].appearance, Appearance::Dark);
}

#[test]
fn removed_entries_dematerialize_and_prune_and_render_on_gpu() {
    let frame = solid_frame(256, 160, [0.35, 0.45, 0.65, 1.0]);
    let mut container = GlassContainer::new();

    let mut a = GlassEntry::new(1, rect(24.0, 48.0, 96.0, 56.0, 28.0));
    a.smoothness = 12.0;
    a.tint = Some([0.1, 0.5, 1.0, 0.35]);
    let mut b = GlassEntry::new(2, rect(128.0, 48.0, 56.0, 56.0, 28.0));
    b.smoothness = 12.0;
    container.submit(&[a, b], 0.0).unwrap();

    // Full GPU render
    let mut renderer = Renderer::new(Size::new(256, 160).unwrap()).unwrap();
    let out = container.render(&mut renderer, &frame, 0.0).unwrap();
    assert_eq!(out.pixels().len(), 256 * 160);
    assert!(out.pixels().iter().flatten().all(|v| v.is_finite()));

    // Remove entry 2 -> dematerializes then prunes after settling at 60 Hz
    container.submit(&[a], 0.0).unwrap();
    assert_eq!(container.len(), 2);
    for step in 1..=120 {
        container.advance(step as f64 / 60.0).unwrap();
    }
    assert_eq!(container.len(), 1);
}
