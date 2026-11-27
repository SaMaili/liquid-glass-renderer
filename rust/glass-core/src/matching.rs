//! General automatic material and shape matching for Liquid Glass containers.
//!
//! Mirrors the macOS `GlassEffectContainer` / `glassEffectID` model:
//! - Persistent `id` entries smoothly retarget geometry and material springs in place.
//! - Cross-ID `match_id` handoff transfers live spring state (position, size, radii,
//!   velocity, tint, diffusion, and backdrop luminance) from a departing surface to an
//!   arriving surface for seamless morphing.
//! - Compatible proximate surfaces are automatically grouped into shared `sdfUnion`
//!   passes while incompatible materials or distant surfaces stay in separate ordered passes.
//! - Optional per-surface backdrop luminance adaptation runs automatically via `BackdropState`.

use std::collections::{HashMap, HashSet};

use crate::{
    Result, Renderer,
    frame::Frame,
    interaction::Spring,
    material::{
        BackgroundStyle, linear_fill_matrix,
        adaptive::{BackdropState, Profile as AdaptiveProfile, Selection},
        recipe::{PreparedSdrMaterial, RecipeRole, SdrRecipe},
        spec::Dimensions,
    },
    overlay::{Appearance, decode},
    raster::{PixelRect, Viewport},
    scene::{Backdrop, Group, MaterialKind, Shape, ShapeFilter, Tint, TintRamp},
    shape::{Rectangle, affine_rectangle},
};

/// Default macOS Golden Gate container spring `[response_sec, damping_fraction]`.
pub const DEFAULT_SPRING: [f64; 2] = [0.40, 0.70];

/// Declarative target description for a single glass surface in a `GlassContainer`.
#[derive(Clone, Copy)]
pub struct GlassEntry {
    /// Persistent logical identity for this surface.
    pub id: u64,
    /// Optional shared morph identity (like SwiftUI `glassEffectID`). When an entry with
    /// this `match_id` disappears and another appears, spring state and backdrop adaptation
    /// transfer automatically.
    pub match_id: Option<u64>,
    /// Compositing order (lower renders first).
    pub z_index: i32,
    /// Target shape in logical points.
    pub rect: Rectangle,
    /// Material recipe role.
    pub role: RecipeRole,
    /// Preferred environment appearance (or explicit appearance when `adaptive == false`).
    pub appearance: Appearance,
    /// Enable automatic backdrop-luminance appearance adaptation (`BackdropState`).
    pub adaptive: bool,
    /// Background diffusion / legibility (`0.0..=1.0`).
    pub diffusion: f32,
    /// Optional straight sRGB tint `[r, g, b, a]` (`0.0..=1.0`).
    pub tint: Option<[f32; 4]>,
    /// Surface opacity (`0.0..=1.0`).
    pub opacity: f32,
    /// Smooth-union (`sdfUnion`) radius in logical points when grouped with compatible
    /// neighboring entries (`0.0` disables merging).
    pub smoothness: f32,
    /// Retargetable spring `[response_sec, damping_fraction]`.
    pub spring: [f64; 2],
}

impl GlassEntry {
    pub fn new(id: u64, rect: Rectangle) -> Self {
        Self {
            id,
            match_id: None,
            z_index: 0,
            rect,
            role: RecipeRole::Regular,
            appearance: Appearance::Dark,
            adaptive: false,
            diffusion: 0.5,
            tint: None,
            opacity: 1.0,
            smoothness: 0.0,
            spring: DEFAULT_SPRING,
        }
    }

    fn validate(&self) -> Result<()> {
        if !self.rect.origin.iter().all(|v| v.is_finite())
            || !self.rect.size.iter().all(|v| v.is_finite() && *v > 0.0)
            || !self.rect.radii.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0e6)
            || !self.diffusion.is_finite() || !(0.0..=1.0).contains(&self.diffusion)
            || !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity)
            || !self.smoothness.is_finite() || self.smoothness < 0.0
            || !self.spring[0].is_finite() || self.spring[0] <= 0.0
            || !self.spring[1].is_finite() || self.spring[1] < 0.0
        {
            return Err("invalid glass container entry".into());
        }
        if let Some(tint) = self.tint {
            if !tint.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                return Err("invalid glass container entry tint".into());
            }
        }
        Ok(())
    }
}

/// Resolved live state of a single matched entry at the current timestamp.
#[derive(Clone, Copy)]
pub struct ResolvedEntry {
    pub id: u64,
    pub match_id: Option<u64>,
    pub z_index: i32,
    pub rect: Rectangle,
    pub role: RecipeRole,
    pub appearance: Appearance,
    pub selection: Selection,
    pub diffusion: f32,
    pub tint: Option<[f32; 4]>,
    pub opacity: f32,
    pub smoothness: f32,
}

/// Automatically grouped render batch sharing one compatible material and `sdfUnion` pass.
pub struct ResolvedGroup {
    pub role: RecipeRole,
    pub appearance: Appearance,
    pub smoothness: f32,
    pub opacity: f32,
    pub entries: Vec<ResolvedEntry>,
    pub material: PreparedSdrMaterial,
    pub tints: Vec<Tint>,
    pub shape_tags: Vec<u64>,
}

struct LiveEntry {
    id: u64,
    match_id: Option<u64>,
    z_index: i32,
    role: RecipeRole,
    appearance: Appearance,
    adaptive: bool,
    continuous: bool,
    smoothness: f32,
    spring: [f64; 2],
    departing: bool,

    // Target values
    target_origin: [f64; 2],
    target_size: [f64; 2],
    target_radii: [f64; 4],
    target_diffusion: f64,
    target_opacity: f64,
    target_tint: [f64; 4],

    // Animated spring states
    x: Spring,
    y: Spring,
    w: Spring,
    h: Spring,
    radii: [Spring; 4],
    diffusion: Spring,
    opacity: Spring,
    tint: [Spring; 4],

    // Current sampled values
    cur_origin: [f64; 2],
    cur_size: [f64; 2],
    cur_radii: [f64; 4],
    cur_diffusion: f64,
    cur_opacity: f64,
    cur_tint: [f64; 4],

    backdrop: BackdropState,
}

impl LiveEntry {
    fn spawn(entry: &GlassEntry, time: f64, materialize: bool) -> Result<Self> {
        let scale = if materialize { 0.92 } else { 1.0 };
        let sw = entry.rect.size[0] * scale;
        let sh = entry.rect.size[1] * scale;
        let sx = entry.rect.origin[0] + (entry.rect.size[0] - sw) * 0.5;
        let sy = entry.rect.origin[1] + (entry.rect.size[1] - sh) * 0.5;
        let sr = entry.rect.radii.map(|r| r * scale);
        let init_opacity = if materialize { 0.0 } else { f64::from(entry.opacity) };
        let tint = entry.tint.unwrap_or([0.0; 4]).map(f64::from);

        Ok(Self {
            id: entry.id,
            match_id: entry.match_id,
            z_index: entry.z_index,
            role: entry.role,
            appearance: entry.appearance,
            adaptive: entry.adaptive,
            continuous: entry.rect.continuous,
            smoothness: entry.smoothness,
            spring: entry.spring,
            departing: false,
            target_origin: entry.rect.origin,
            target_size: entry.rect.size,
            target_radii: entry.rect.radii,
            target_diffusion: f64::from(entry.diffusion),
            target_opacity: f64::from(entry.opacity),
            target_tint: tint,
            x: Spring::new(sx, time)?,
            y: Spring::new(sy, time)?,
            w: Spring::new(sw, time)?,
            h: Spring::new(sh, time)?,
            radii: [
                Spring::new(sr[0], time)?,
                Spring::new(sr[1], time)?,
                Spring::new(sr[2], time)?,
                Spring::new(sr[3], time)?,
            ],
            diffusion: Spring::new(f64::from(entry.diffusion), time)?,
            opacity: Spring::new(init_opacity, time)?,
            tint: [
                Spring::new(tint[0], time)?,
                Spring::new(tint[1], time)?,
                Spring::new(tint[2], time)?,
                Spring::new(tint[3], time)?,
            ],
            cur_origin: [sx, sy],
            cur_size: [sw, sh],
            cur_radii: sr,
            cur_diffusion: f64::from(entry.diffusion),
            cur_opacity: init_opacity,
            cur_tint: tint,
            backdrop: BackdropState::default(),
        })
    }

    fn retarget(&mut self, entry: &GlassEntry) {
        self.id = entry.id;
        self.match_id = entry.match_id;
        self.z_index = entry.z_index;
        self.role = entry.role;
        self.appearance = entry.appearance;
        self.adaptive = entry.adaptive;
        self.continuous = entry.rect.continuous;
        self.smoothness = entry.smoothness;
        self.spring = entry.spring;
        self.departing = false;
        self.target_origin = entry.rect.origin;
        self.target_size = entry.rect.size;
        self.target_radii = entry.rect.radii;
        self.target_diffusion = f64::from(entry.diffusion);
        self.target_opacity = f64::from(entry.opacity);
        self.target_tint = entry.tint.unwrap_or([0.0; 4]).map(f64::from);
    }

    fn mark_departing(&mut self) {
        self.departing = true;
        self.target_opacity = 0.0;
    }

    fn advance(&mut self, time: f64) -> Result<bool> {
        let [response, damping] = self.spring;
        let mut moving = false;

        let mut step = |s: &mut Spring, target: f64, out: &mut f64| -> Result<()> {
            if let Some(val) = s.advance(target, time, response, damping)? {
                *out = val;
                moving = true;
            } else {
                *out = target;
            }
            Ok(())
        };

        step(&mut self.x, self.target_origin[0], &mut self.cur_origin[0])?;
        step(&mut self.y, self.target_origin[1], &mut self.cur_origin[1])?;
        step(&mut self.w, self.target_size[0], &mut self.cur_size[0])?;
        step(&mut self.h, self.target_size[1], &mut self.cur_size[1])?;
        for i in 0..4 {
            step(&mut self.radii[i], self.target_radii[i], &mut self.cur_radii[i])?;
        }
        step(&mut self.diffusion, self.target_diffusion, &mut self.cur_diffusion)?;
        step(&mut self.opacity, self.target_opacity, &mut self.cur_opacity)?;
        for i in 0..4 {
            step(&mut self.tint[i], self.target_tint[i], &mut self.cur_tint[i])?;
        }
        Ok(moving)
    }

    fn current_rect(&self) -> Rectangle {
        Rectangle {
            origin: self.cur_origin,
            size: [self.cur_size[0].max(1.0), self.cur_size[1].max(1.0)],
            radii: self.cur_radii.map(|r| r.max(0.0)),
            continuous: self.continuous,
        }
    }

    fn current_tint(&self) -> Option<[f32; 4]> {
        let a = (self.cur_tint[3] as f32).clamp(0.0, 1.0);
        if a <= 0.001 {
            None
        } else {
            Some([
                (self.cur_tint[0] as f32).clamp(0.0, 1.0),
                (self.cur_tint[1] as f32).clamp(0.0, 1.0),
                (self.cur_tint[2] as f32).clamp(0.0, 1.0),
                a,
            ])
        }
    }
}

/// Automatic material and shape matching container.
///
/// Submit declarative `GlassEntry` lists on each state change; the container
/// automatically matches identities (`id` and `match_id`), retargets springs,
/// adapts appearance to backdrop luminance, groups compatible proximate shapes
/// into `sdfUnion` batches, and renders through `LiveScene`.
pub struct GlassContainer {
    entries: HashMap<u64, LiveEntry>,
    initialized: bool,
    adaptive_profile: AdaptiveProfile,
}

impl Default for GlassContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl GlassContainer {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            initialized: false,
            adaptive_profile: AdaptiveProfile {
                adaptive_v2_enabled: true,
                ..AdaptiveProfile::default()
            },
        }
    }

    /// Override the backdrop luminance adaptation profile (hysteresis, reduce_transparency, etc.).
    pub fn set_adaptive_profile(&mut self, profile: AdaptiveProfile) {
        self.adaptive_profile = profile;
    }

    /// Number of currently live (active + dematerializing) entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Submit the desired set of glass entries at `time` (in seconds).
    ///
    /// On the very first `submit`, entries snap immediately to their initial state.
    /// Subsequent submissions automatically match by `id`, hand off spring + backdrop
    /// state across `match_id`, materialize new entries, and dematerialize removed ones.
    pub fn submit(&mut self, desired: &[GlassEntry], time: f64) -> Result<()> {
        let mut seen_ids = HashSet::with_capacity(desired.len());
        for entry in desired {
            entry.validate()?;
            if !seen_ids.insert(entry.id) {
                return Err(format!("duplicate glass entry id {}", entry.id));
            }
        }

        // First submission initializes directly without materialization fade-in.
        if !self.initialized {
            for entry in desired {
                self.entries.insert(entry.id, LiveEntry::spawn(entry, time, false)?);
            }
            self.initialized = true;
            return Ok(());
        }

        // Collect departing entries indexed by match_id so arriving entries can inherit their state.
        let mut handoff_by_match: HashMap<u64, LiveEntry> = HashMap::new();
        let departing_ids: Vec<u64> = self.entries.keys()
            .copied()
            .filter(|id| !seen_ids.contains(id))
            .collect();

        // Reserve departing entries that have a match_id claimed by a brand-new desired entry.
        let new_match_ids: HashSet<u64> = desired.iter()
            .filter(|e| !self.entries.contains_key(&e.id))
            .filter_map(|e| e.match_id)
            .collect();

        for id in departing_ids {
            let can_handoff = self.entries.get(&id)
                .and_then(|e| e.match_id)
                .is_some_and(|mid| new_match_ids.contains(&mid) && !handoff_by_match.contains_key(&mid));
            if can_handoff {
                let live = self.entries.remove(&id).unwrap();
                handoff_by_match.insert(live.match_id.unwrap(), live);
            } else if let Some(live) = self.entries.get_mut(&id) {
                live.mark_departing();
            }
        }

        // Update matched entries, hand off cross-ID matches, or materialize new entries.
        for entry in desired {
            if let Some(live) = self.entries.get_mut(&entry.id) {
                live.retarget(entry);
            } else if let Some(mut inherited) = entry.match_id.and_then(|mid| handoff_by_match.remove(&mid)) {
                inherited.retarget(entry);
                self.entries.insert(entry.id, inherited);
            } else {
                self.entries.insert(entry.id, LiveEntry::spawn(entry, time, true)?);
            }
        }

        // Any unclaimed handoff entries continue dematerializing gracefully.
        for (_, mut leftover) in handoff_by_match {
            leftover.mark_departing();
            self.entries.insert(leftover.id, leftover);
        }

        Ok(())
    }

    /// Advance all geometry and material springs to `time`. Returns `true` if any spring is still moving.
    pub fn advance(&mut self, time: f64) -> Result<bool> {
        let mut any_moving = false;
        let mut remove_ids = Vec::new();
        for (&id, entry) in &mut self.entries {
            let moving = entry.advance(time)?;
            if moving {
                any_moving = true;
            } else if entry.departing && entry.cur_opacity <= 0.005 {
                remove_ids.push(id);
            }
        }
        for id in remove_ids {
            self.entries.remove(&id);
        }
        Ok(any_moving)
    }

    /// Resolve backdrop-adapted appearances, group compatible proximate shapes into
    /// shared `sdfUnion` batches, and build their `PreparedSdrMaterial` payloads.
    pub fn resolve(&mut self, source: &Frame) -> Result<Vec<ResolvedGroup>> {
        let viewport = source.viewport();
        let mut ids: Vec<u64> = self.entries.keys().copied().collect();
        ids.sort_by_key(|id| {
            let e = &self.entries[id];
            (e.z_index, e.id)
        });

        let mut resolved = Vec::with_capacity(ids.len());
        for id in ids {
            let entry = self.entries.get_mut(&id).unwrap();
            let opacity = (entry.cur_opacity as f32).clamp(0.0, 1.0);
            if opacity <= 0.002 {
                continue;
            }
            let rect = entry.current_rect();
            let diffusion = (entry.cur_diffusion as f32).clamp(0.0, 1.0);
            let tint = entry.current_tint();
            let dims = dimensions_for_rect(rect);

            let can_adapt = entry.adaptive
                && matches!(entry.role, RecipeRole::Regular | RecipeRole::Clear | RecipeRole::Dock);
            let selection = if can_adapt {
                let probe_recipe = SdrRecipe {
                    role: entry.role,
                    appearance: entry.appearance,
                    dimensions: dims,
                    diffusion,
                    tint: None,
                    style: BackgroundStyle::default(),
                };
                let obs = observation_rect(viewport, rect);
                entry.backdrop.update(&probe_recipe, self.adaptive_profile, 0, source, obs)?
            } else {
                Selection { active: false, appearance: entry.appearance, luminance: None }
            };

            resolved.push(ResolvedEntry {
                id: entry.id,
                match_id: entry.match_id,
                z_index: entry.z_index,
                rect,
                role: entry.role,
                appearance: selection.appearance,
                selection,
                diffusion,
                tint,
                opacity,
                smoothness: entry.smoothness,
            });
        }

        // Partition sorted entries into compatible `sdfUnion` groups.
        let mut raw_groups: Vec<Vec<ResolvedEntry>> = Vec::new();
        for item in resolved {
            let can_merge = raw_groups.last().is_some_and(|group| {
                group.iter().any(|existing| compatible_for_merge(existing, &item))
            });
            if can_merge {
                raw_groups.last_mut().unwrap().push(item);
            } else {
                raw_groups.push(vec![item]);
            }
        }

        let mut out = Vec::with_capacity(raw_groups.len());
        for entries in raw_groups {
            let first = entries[0];
            let smoothness = if entries.len() > 1 {
                entries.iter().map(|e| e.smoothness).fold(0.0_f32, f32::max)
            } else {
                0.0
            };
            let avg_diffusion = entries.iter().map(|e| e.diffusion).sum::<f32>() / entries.len() as f32;
            let group_opacity = entries.iter().map(|e| e.opacity).fold(0.0_f32, f32::max);
            let dims = dimensions_for_group(&entries);
            let group_luma = entries.iter().find_map(|e| e.selection.luminance);

            // When all shapes share no tint or a single shape carries the tint, fold it into
            // the recipe; for multi-shape groups with distinct tints, attach tagged Tint passes.
            let multi_tinted = entries.len() > 1 && entries.iter().any(|e| e.tint.is_some());
            let recipe_tint = if !multi_tinted { first.tint } else { None };
            let recipe = SdrRecipe {
                role: first.role,
                appearance: first.appearance,
                dimensions: dims,
                diffusion: avg_diffusion,
                tint: recipe_tint,
                style: BackgroundStyle::default(),
            };
            let material = recipe.build_with_luminance(viewport, group_luma)?;

            let mut shape_tags = Vec::new();
            let mut tints = Vec::new();
            if multi_tinted {
                for (idx, item) in entries.iter().enumerate() {
                    let tag = (idx as u64) + 1;
                    shape_tags.push(tag);
                    if let Some(rgba) = item.tint {
                        let a = rgba[3];
                        let linear = decode([rgba[0] * a, rgba[1] * a, rgba[2] * a, a]);
                        tints.push(Tint {
                            shape_filter: ShapeFilter::exact(tag),
                            ramp: TintRamp::SOFT,
                            opacity: item.opacity,
                            matrix: linear_fill_matrix(linear),
                            max_color_component: 1.0,
                            preserves_hue: false,
                        });
                    }
                }
            }

            out.push(ResolvedGroup {
                role: first.role,
                appearance: first.appearance,
                smoothness,
                opacity: group_opacity,
                entries,
                material,
                tints,
                shape_tags,
            });
        }

        Ok(out)
    }

    /// Advance springs to `time`, resolve adapted materials and groups, and render the
    /// entire container into a new composited `Frame`.
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        source: &Frame,
        time: f64,
    ) -> Result<Frame> {
        self.advance(time)?;
        let groups = self.resolve(source)?;
        let viewport = source.viewport();
        let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

        let mut shape_vecs: Vec<Vec<Shape<'static>>> = Vec::with_capacity(groups.len());
        for group in &groups {
            let mut shapes = Vec::with_capacity(group.entries.len());
            for entry in &group.entries {
                shapes.push(affine_rectangle(viewport, entry.rect, identity)?);
            }
            shape_vecs.push(shapes);
        }

        let mut scene_groups = Vec::with_capacity(groups.len());
        for (group, shapes) in groups.iter().zip(&shape_vecs) {
            scene_groups.push(Group {
                shapes,
                shape_tags: &group.shape_tags,
                material: group.material.material.material(MaterialKind::All)?,
                tints: &group.tints,
                opacity: group.opacity,
                smoothness: group.smoothness,
                blur_levels: group.material.blur_levels,
            });
        }

        let mut out = Frame::new(viewport, source.pixels().to_vec())?;
        out.render_groups(renderer, Backdrop::Current, &scene_groups)?;
        Ok(out)
    }
}

fn compatible_for_merge(a: &ResolvedEntry, b: &ResolvedEntry) -> bool {
    if a.role != b.role || a.appearance != b.appearance {
        return false;
    }
    let smooth = a.smoothness.max(b.smoothness);
    if smooth <= 0.0 {
        return false;
    }
    if (a.diffusion - b.diffusion).abs() > 0.05 {
        return false;
    }
    let gap = rect_gap(a.rect, b.rect);
    gap <= f64::from(smooth) * 2.5
}

fn rect_gap(a: Rectangle, b: Rectangle) -> f64 {
    let dx = (a.origin[0] - (b.origin[0] + b.size[0]))
        .max(b.origin[0] - (a.origin[0] + a.size[0]))
        .max(0.0);
    let dy = (a.origin[1] - (b.origin[1] + b.size[1]))
        .max(b.origin[1] - (a.origin[1] + a.size[1]))
        .max(0.0);
    (dx * dx + dy * dy).sqrt()
}

fn dimensions_for_rect(rect: Rectangle) -> Dimensions {
    let min_side = rect.size[0].min(rect.size[1]);
    Dimensions {
        minimum: Some(min_side),
        largest_area_minimum: Some(min_side),
    }
}

fn dimensions_for_group(entries: &[ResolvedEntry]) -> Dimensions {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut largest_min = 0.0_f64;
    for e in entries {
        min_x = min_x.min(e.rect.origin[0]);
        min_y = min_y.min(e.rect.origin[1]);
        max_x = max_x.max(e.rect.origin[0] + e.rect.size[0]);
        max_y = max_y.max(e.rect.origin[1] + e.rect.size[1]);
        largest_min = largest_min.max(e.rect.size[0].min(e.rect.size[1]));
    }
    let group_min = (max_x - min_x).max(1.0).min((max_y - min_y).max(1.0));
    Dimensions {
        minimum: Some(group_min),
        largest_area_minimum: Some(largest_min),
    }
}

fn observation_rect(viewport: Viewport, rect: Rectangle) -> Option<PixelRect> {
    let scale = f64::from(viewport.scale().factor());
    let raster = viewport.pixels();
    let x0 = (rect.origin[0] * scale).floor().clamp(0.0, f64::from(raster.width)) as u32;
    let y0 = (rect.origin[1] * scale).floor().clamp(0.0, f64::from(raster.height)) as u32;
    let x1 = ((rect.origin[0] + rect.size[0]) * scale).ceil().clamp(0.0, f64::from(raster.width)) as u32;
    let y1 = ((rect.origin[1] + rect.size[1]) * scale).ceil().clamp(0.0, f64::from(raster.height)) as u32;
    if x1 > x0 && y1 > y0 {
        Some(PixelRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 })
    } else {
        None
    }
}
