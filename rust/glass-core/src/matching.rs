//! Authentic `26A434` automatic material and shape matching (`GlassContainer`).
//!
//! Implements the retail macOS `DesignLibrary` / `AppKit` / `CoreAnimation` container rules
//! (`gaussian_bridge_enabled = false`, `0x277f74c28 == 0`):
//! - **Spatial `sourceElement` matching (`0x22453991c` & `0x22487fff0`)**: automatic
//!   edge-gap matching (`CGRect.minDistance` with `FRINTA` ties-to-even pixel rounding),
//!   directed factor-3 size gate (`target < 3 * candidate`), `hasCloseElement` (`0x22453fda4`)
//!   stepping-stone chains, and competing-target override (`0x22453fb88`).
//! - **`matchedGeometry` Mitosis Emergence & Absorption (`0x2245362c4` + `sdf-union.frag`)**:
//!   arriving matched shapes start tucked inside the facing rim of their source surface and
//!   emerge via `0.50s / 0.70` fluid springs through the container's `spacing` (`smoothness`),
//!   forming a true curved hyperbolic meniscus neck in `sdf-union.frag` that pinches off
//!   cleanly like real liquid; departing matched shapes retract back inside their partner.
//! - **`ScalePulse` & `TranslationKick` (`0x2245362c4`)**: subtle staggered squash-and-stretch
//!   (`0.135s` height / `0.170s` width return) and recoil on the partner surface.
//! - **`NativeGlassAppearance` (`0x224ba9d94`, `0x22519bcb0`)**: anisotropic `+16 pt`-capped
//!   vapor dispersion (`(1.2, 16, 16, 10)`) for isolated unmatched materialization.
//! - **Conditional Z-sort & contiguous `DisplayMaterial` chunking (`0x185df53c4`, `0x185ded578`)**:
//!   skips sorting when all material identities match, preserves strict painter's order across
//!   intervening layers, and delegates smooth-union continuously to GPU `sdf-union.frag`.
//! - **Extension beyond Apple `26A434` (per-entry adaptive convergence + dynamic union)**:
//!   in macOS (`HINTERGRUNDANPASSUNG.md`), a `GlassEffectContainer` shares a single material
//!   and observation rect across all its members (so two elements cannot simultaneously adapt
//!   independently to `Light` and `Dark` and later merge into one `sdfUnion`). `GlassContainer`
//!   extends this by allowing each entry in a `group_id` to adapt its own `BackdropState`
//!   independently (`Animation.default` `FluidSpring(0.50, 1.00)`) and dynamically uniting
//!   them into a shared `sdfUnion` pass once their `dark_mix` converges within `<= 0.005`
//!   (sub-`1/255` sRGB delta, preventing any brightness flash upon group join).

use std::collections::{HashMap, HashSet};

use crate::{
    Renderer, Result,
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

/// Default `26A434` `transition_defaults` matched spring `[response_sec, damping_fraction]`.
pub const DEFAULT_SPRING: [f64; 2] = [0.50, 0.70];

/// Default `26A434` `transition_defaults` materialize spring `[response_sec, damping_fraction]`.
pub const MATERIALIZE_SPRING: [f64; 2] = [0.50, 1.00];

/// Default maximum edge-gap distance in logical points for automatic spatial matching.
pub const DEFAULT_MAX_MATCH_DISTANCE: f64 = 96.0;

/// Declarative target description for a single glass surface in a `GlassContainer`.
#[derive(Clone, Copy)]
pub struct GlassEntry {
    /// Persistent logical identity for this surface.
    pub id: u64,
    /// Optional explicit morph identity (like SwiftUI `glassEffectID`). When `None`,
    /// `GlassContainer` performs automatic spatial `sourceElement` matching within
    /// `max_match_distance` unless `allows_matching` is `false`.
    pub match_id: Option<u64>,
    /// Optional explicit namespace / backdrop group tag (`0x185df53c4`). Entries with
    /// different `group_id` never merge into the same `sdfUnion` pass.
    pub group_id: u64,
    /// Compositing order (lower renders first when mixed material identities trigger Z-sort).
    pub z_index: i32,
    /// Target shape in logical points.
    pub rect: Rectangle,
    /// Material recipe role.
    pub role: RecipeRole,
    /// Preferred environment appearance (or explicit appearance when `adaptive == false`).
    pub appearance: Appearance,
    /// Enable automatic `26A434` V2 backdrop-luminance adaptation (`BackdropState`).
    pub adaptive: bool,
    /// Allow automatic spatial `sourceElement` matching (liquid mitosis emergence / absorption).
    pub allows_matching: bool,
    /// Background diffusion / legibility (`0.0..=1.0`).
    pub diffusion: f32,
    /// Optional straight sRGB tint `[r, g, b, a]` (`0.0..=1.0`).
    pub tint: Option<[f32; 4]>,
    /// Surface opacity (`0.0..=1.0`).
    pub opacity: f32,
    /// Container `spacing` / `sdfUnion` smoothness radius in logical points (`NSGlassEffectContainerView.spacing`).
    /// Shapes in the same `DisplayMaterial` chunk form a curved liquid neck when their gap is
    /// smaller than `smoothness / 2` and stand 100% undeformed once their gap exceeds `smoothness`.
    pub smoothness: f32,
    /// Retargetable geometry spring `[response_sec, damping_fraction]`.
    pub spring: [f64; 2],
}

impl GlassEntry {
    pub fn new(id: u64, rect: Rectangle) -> Self {
        Self {
            id,
            match_id: None,
            group_id: 0,
            z_index: 0,
            rect,
            role: RecipeRole::Regular,
            appearance: Appearance::Dark,
            adaptive: false,
            allows_matching: true,
            diffusion: 0.5,
            tint: None,
            opacity: 1.0,
            smoothness: 28.0,
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

/// Exact `26A434` `Entry.geometryResult` (`0x22487fff0`) & `CGRect.minDistance` (`0x2252722b8`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryMatch {
    pub candidate_id: u64,
    pub distance: f64,
    pub overlapping: bool,
    pub min_offset: Option<[f64; 2]>,
    pub anchor: [f64; 2],
}

/// Resolved live state of a single matched entry at the current timestamp.
#[derive(Clone, Copy)]
pub struct ResolvedEntry {
    pub id: u64,
    pub match_id: Option<u64>,
    pub matched_partner_id: Option<u64>,
    pub group_id: u64,
    pub z_index: i32,
    pub rect: Rectangle,
    pub role: RecipeRole,
    pub configured_appearance: Appearance,
    pub appearance: Appearance,
    /// Continuous `0.0` (`Light`) .. `1.0` (`Dark`) mix animated via `26A434`
    /// `Animation.default` (`FluidSpring(0.50, 1.00)`, `0x2246f92e4` / `0x23f5ad784`).
    pub dark_mix: f32,
    pub selection: Selection,
    pub diffusion: f32,
    pub tint: Option<[f32; 4]>,
    pub opacity: f32,
    pub smoothness: f32,
}

/// Contiguous `DisplayMaterial` render batch produced by `GlassContainer::resolve`.
pub struct ResolvedGroup {
    pub role: RecipeRole,
    pub appearance: Appearance,
    pub group_id: u64,
    pub smoothness: f32,
    pub opacity: f32,
    pub entries: Vec<ResolvedEntry>,
    pub material: PreparedSdrMaterial,
    pub tints: Vec<Tint>,
    pub shape_tags: Vec<u64>,
}

/// Delayed `Spring` track matching `NativeGlassScalarTrack` (`FluidSpring.delayed`).
#[derive(Clone, Copy)]
struct DelayedTrack {
    spring: Spring,
    value: f64,
    target: f64,
    response: f64,
    damping: f64,
    activate_at: f64,
}

impl DelayedTrack {
    fn new(initial: f64, time: f64) -> Result<Self> {
        Ok(Self {
            spring: Spring::new(initial, time)?,
            value: initial,
            target: initial,
            response: 0.5,
            damping: 1.0,
            activate_at: time,
        })
    }

    fn trigger_delayed(
        &mut self,
        target: f64,
        response: f64,
        damping: f64,
        activate_at: f64,
    ) -> Result<()> {
        self.spring = Spring::new(self.value, activate_at)?;
        self.target = target;
        self.response = response;
        self.damping = damping;
        self.activate_at = activate_at;
        Ok(())
    }

    fn retarget_at(
        &mut self,
        target: f64,
        response: f64,
        damping: f64,
        at_time: f64,
    ) -> Result<()> {
        if at_time > self.activate_at {
            let _ = self.advance(at_time)?;
        }
        self.target = target;
        self.response = response;
        self.damping = damping;
        self.activate_at = at_time;
        Ok(())
    }

    fn advance(&mut self, time: f64) -> Result<bool> {
        if time <= self.activate_at {
            return Ok(true);
        }
        if let Some(v) = self.spring.advance(self.target, time, self.response, self.damping)? {
            self.value = v;
            Ok(true)
        } else {
            self.value = self.target;
            Ok(false)
        }
    }
}

/// `26A434` `ScalePulse` + `TranslationKick` state (`scale_pulse` 184B + `translation_kick` 88B).
struct EffectLeaves {
    anchor: [f64; 2],
    pulse_gain: f64,
    up: DelayedTrack,
    down_h: DelayedTrack,
    down_w: DelayedTrack,
    kick_x: DelayedTrack,
    kick_y: DelayedTrack,
    kick_return_at: Option<f64>,
}

impl EffectLeaves {
    fn new(time: f64) -> Result<Self> {
        Ok(Self {
            anchor: [0.5, 0.5],
            pulse_gain: 1.0,
            up: DelayedTrack::new(0.0, time)?,
            down_h: DelayedTrack::new(1.0, time)?,
            down_w: DelayedTrack::new(1.0, time)?,
            kick_x: DelayedTrack::new(0.0, time)?,
            kick_y: DelayedTrack::new(0.0, time)?,
            kick_return_at: None,
        })
    }

    fn trigger_pulse(&mut self, anchor: [f64; 2], gain: f64, time: f64) -> Result<()> {
        self.anchor = anchor;
        self.pulse_gain = gain;
        self.up = DelayedTrack::new(0.0, time)?;
        self.up.trigger_delayed(1.0, 0.35, 0.40, time)?;
        self.down_h = DelayedTrack::new(1.0, time)?;
        self.down_h.trigger_delayed(0.0, 0.50, 0.50, time + 0.135)?;
        self.down_w = DelayedTrack::new(1.0, time)?;
        self.down_w.trigger_delayed(0.0, 0.50, 0.40, time + 0.170)?;
        Ok(())
    }

    fn trigger_kick(&mut self, offset: [f64; 2], max_pts: f64, time: f64) -> Result<()> {
        let len = offset[0].hypot(offset[1]);
        if len <= 1.0e-6 {
            return Ok(());
        }
        let amp = len.min(max_pts) / len;
        let kx = offset[0] * amp;
        let ky = offset[1] * amp;
        self.kick_x.trigger_delayed(kx, 0.30, 1.00, time + 0.06)?;
        self.kick_y.trigger_delayed(ky, 0.30, 1.00, time + 0.06)?;
        self.kick_return_at = Some(time + 0.18);
        Ok(())
    }

    fn advance(&mut self, time: f64) -> Result<bool> {
        if let Some(ret_t) = self.kick_return_at {
            if time >= ret_t {
                self.kick_x.retarget_at(0.0, 0.60, 0.80, ret_t)?;
                self.kick_y.retarget_at(0.0, 0.60, 0.80, ret_t)?;
                self.kick_return_at = None;
            }
        }
        let mut moving = self.kick_return_at.is_some();
        moving |= self.up.advance(time)?;
        moving |= self.down_h.advance(time)?;
        moving |= self.down_w.advance(time)?;
        moving |= self.kick_x.advance(time)?;
        moving |= self.kick_y.advance(time)?;
        Ok(moving)
    }

    fn apply(&self, mut rect: Rectangle) -> Rectangle {
        let uw = (self.up.value * self.down_w.value).clamp(-0.5, 2.0) * self.pulse_gain;
        let uh = (self.up.value * self.down_h.value).clamp(-0.5, 2.0) * self.pulse_gain;
        let dw = (0.44 * rect.size[0]).min(16.0) * uw;
        let dh = (0.44 * rect.size[1]).min(16.0) * uh;
        rect.origin[0] += self.kick_x.value - dw * self.anchor[0];
        rect.origin[1] += self.kick_y.value - dh * self.anchor[1];
        rect.size[0] = (rect.size[0] + dw).max(1.0);
        rect.size[1] = (rect.size[1] + dh).max(1.0);
        rect
    }
}

struct LiveEntry {
    id: u64,
    match_id: Option<u64>,
    matched_partner_id: Option<u64>,
    group_id: u64,
    order_index: usize,
    z_index: i32,
    role: RecipeRole,
    appearance: Appearance,
    adaptive: bool,
    allows_matching: bool,
    continuous: bool,
    smoothness: f32,
    spring: [f64; 2],
    departing: bool,
    matched_bud: bool,
    appearance_anchor: [f64; 2],
    fade_after: f64,

    target_origin: [f64; 2],
    target_size: [f64; 2],
    target_radii: [f64; 4],
    target_diffusion: f64,
    target_opacity: f64,
    target_tint: [f64; 4],
    target_scale_progress: f64,
    target_dark_mix: f64,
    target_luma: Option<f64>,

    x: Spring,
    y: Spring,
    w: Spring,
    h: Spring,
    radii: [Spring; 4],
    diffusion: Spring,
    opacity: Spring,
    scale_progress: Spring,
    dark_mix: Spring,
    luma_spring: Option<Spring>,
    tint: [Spring; 4],

    cur_origin: [f64; 2],
    cur_size: [f64; 2],
    cur_radii: [f64; 4],
    cur_diffusion: f64,
    cur_opacity: f64,
    cur_scale_progress: f64,
    cur_dark_mix: f64,
    cur_luma: Option<f32>,
    cur_tint: [f64; 4],

    last_advance_time: f64,
    backdrop_initialized: bool,
    effects: EffectLeaves,
    backdrop: BackdropState,
}

impl LiveEntry {
    fn spawn(
        entry: &GlassEntry,
        order_index: usize,
        time: f64,
        materialize: bool,
        spatial_match: Option<(GeometryMatch, Rectangle)>,
    ) -> Result<Self> {
        let tint = entry.tint.unwrap_or([0.0; 4]).map(f64::from);
        let effects = EffectLeaves::new(time)?;

        // When matched via `sourceElement`, start tucked inside the partner's facing rim
        // at full glass opacity so the droplet physically bulges out of the partner's body,
        // stretches a curved hyperbolic meniscus in `sdf-union.frag`, and pinches off cleanly!
        // When unmatched, materialize in place with `NativeGlassAppearance` `(1.2, 16, 16, 10)` vapor dispersion.
        let (start_rect, init_opacity, init_sp, matched_partner_id, matched_bud, anchor) =
            if let Some((m, partner_rect)) = spatial_match {
                let tucked = tucked_rect_inside_partner(partner_rect, entry.rect);
                (
                    tucked,
                    f64::from(entry.opacity),
                    0.0,
                    Some(m.candidate_id),
                    true,
                    m.anchor,
                )
            } else if materialize {
                (entry.rect, 0.0, 1.0, None, false, [0.5, 0.5])
            } else {
                (entry.rect, f64::from(entry.opacity), 0.0, None, false, [0.5, 0.5])
            };

        let init_dark = if matches!(entry.appearance, Appearance::Dark) { 1.0 } else { 0.0 };

        Ok(Self {
            id: entry.id,
            match_id: entry.match_id,
            matched_partner_id,
            group_id: entry.group_id,
            order_index,
            z_index: entry.z_index,
            role: entry.role,
            appearance: entry.appearance,
            adaptive: entry.adaptive,
            allows_matching: entry.allows_matching,
            continuous: entry.rect.continuous,
            smoothness: entry.smoothness,
            spring: entry.spring,
            departing: false,
            matched_bud,
            appearance_anchor: anchor,
            fade_after: time,
            target_origin: entry.rect.origin,
            target_size: entry.rect.size,
            target_radii: entry.rect.radii,
            target_diffusion: f64::from(entry.diffusion),
            target_opacity: f64::from(entry.opacity),
            target_tint: tint,
            target_scale_progress: 0.0,
            target_dark_mix: init_dark,
            target_luma: None,
            x: Spring::new(start_rect.origin[0], time)?,
            y: Spring::new(start_rect.origin[1], time)?,
            w: Spring::new(start_rect.size[0], time)?,
            h: Spring::new(start_rect.size[1], time)?,
            radii: [
                Spring::new(start_rect.radii[0], time)?,
                Spring::new(start_rect.radii[1], time)?,
                Spring::new(start_rect.radii[2], time)?,
                Spring::new(start_rect.radii[3], time)?,
            ],
            diffusion: Spring::new(f64::from(entry.diffusion), time)?,
            opacity: Spring::new(init_opacity, time)?,
            scale_progress: Spring::new(init_sp, time)?,
            dark_mix: Spring::new(init_dark, time)?,
            luma_spring: None,
            tint: [
                Spring::new(tint[0], time)?,
                Spring::new(tint[1], time)?,
                Spring::new(tint[2], time)?,
                Spring::new(tint[3], time)?,
            ],
            cur_origin: start_rect.origin,
            cur_size: start_rect.size,
            cur_radii: start_rect.radii,
            cur_diffusion: f64::from(entry.diffusion),
            cur_opacity: init_opacity,
            cur_scale_progress: init_sp,
            cur_dark_mix: init_dark,
            cur_luma: None,
            cur_tint: tint,
            last_advance_time: time,
            backdrop_initialized: !entry.adaptive,
            effects,
            backdrop: BackdropState::default(),
        })
    }

    fn retarget(&mut self, entry: &GlassEntry, order_index: usize) {
        self.id = entry.id;
        self.match_id = entry.match_id;
        self.group_id = entry.group_id;
        self.order_index = order_index;
        self.z_index = entry.z_index;
        self.role = entry.role;
        self.appearance = entry.appearance;
        self.adaptive = entry.adaptive;
        self.allows_matching = entry.allows_matching;
        self.continuous = entry.rect.continuous;
        self.smoothness = entry.smoothness;
        self.spring = entry.spring;
        self.departing = false;
        self.target_origin = entry.rect.origin;
        self.target_size = entry.rect.size;
        self.target_radii = entry.rect.radii;
        self.target_diffusion = f64::from(entry.diffusion);
        self.target_opacity = f64::from(entry.opacity);
        self.target_scale_progress = 0.0;
        if !entry.adaptive {
            self.target_dark_mix = if matches!(entry.appearance, Appearance::Dark) { 1.0 } else { 0.0 };
        }
        self.target_tint = entry.tint.unwrap_or([0.0; 4]).map(f64::from);
    }

    fn mark_departing(
        &mut self,
        spatial_match: Option<(GeometryMatch, Rectangle)>,
        time: f64,
    ) -> Result<()> {
        if !self.departing {
            self.departing = true;
            if let Some((m, partner_rect)) = spatial_match {
                // Retract physically back inside the matched partner's facing rim so `sdf-union.frag`
                // forms the reverse hyperbolic meniscus fusion on impact, then fade out inside!
                let cur = self.base_rect();
                let tucked = tucked_rect_inside_partner(partner_rect, cur);
                self.matched_partner_id = Some(m.candidate_id);
                self.matched_bud = true;
                self.appearance_anchor = m.anchor;
                self.target_origin = tucked.origin;
                self.target_size = tucked.size;
                self.target_radii = tucked.radii;
                self.spring = [0.46, 0.84];
                self.target_opacity = 0.0;
                self.target_tint[3] = 0.0;
                self.fade_after = time + 0.16;
            } else {
                self.matched_bud = false;
                self.appearance_anchor = [0.5, 0.5];
                self.target_opacity = 0.0;
                self.target_scale_progress = 1.0;
                self.fade_after = time;
            }
        }
        Ok(())
    }

    fn advance(&mut self, time: f64) -> Result<bool> {
        self.last_advance_time = time;
        let [response, damping] = self.spring;
        let [mat_r, mat_d] = if self.matched_bud { [0.28, 1.00] } else { MATERIALIZE_SPRING };
        let mut moving = false;

        let step = |s: &mut Spring, target: f64, r: f64, d: f64, out: &mut f64| -> Result<bool> {
            if let Some(val) = s.advance(target, time, r, d)? {
                *out = val;
                Ok(true)
            } else {
                *out = target;
                Ok(false)
            }
        };

        moving |= step(&mut self.x, self.target_origin[0], response, damping, &mut self.cur_origin[0])?;
        moving |= step(&mut self.y, self.target_origin[1], response, damping, &mut self.cur_origin[1])?;
        moving |= step(&mut self.w, self.target_size[0], response, damping, &mut self.cur_size[0])?;
        moving |= step(&mut self.h, self.target_size[1], response, damping, &mut self.cur_size[1])?;
        for i in 0..4 {
            moving |= step(&mut self.radii[i], self.target_radii[i], response, damping, &mut self.cur_radii[i])?;
        }
        moving |= step(&mut self.diffusion, self.target_diffusion, response, damping, &mut self.cur_diffusion)?;

        // `26A434` `Animation.default` (`FluidSpring(0.50, 1.00)`, `0x2246f92e4` / `0x23f5ad784`)
        // drives `adaptiveAnimations` (`0x40000`) between `Appearance::Light` and `Appearance::Dark`!
        moving |= step(
            &mut self.dark_mix,
            self.target_dark_mix,
            MATERIALIZE_SPRING[0],
            MATERIALIZE_SPRING[1],
            &mut self.cur_dark_mix,
        )?;
        if let (Some(ls), Some(tl)) = (&mut self.luma_spring, self.target_luma) {
            let mut cl = f64::from(self.cur_luma.unwrap_or(tl as f32));
            moving |= step(ls, tl, MATERIALIZE_SPRING[0], MATERIALIZE_SPRING[1], &mut cl)?;
            self.cur_luma = Some(cl as f32);
        }

        if self.departing && time < self.fade_after {
            // Keep opacity spring clock synchronized while droplet travels into partner
            let _ = self.opacity.advance(self.cur_opacity, time, mat_r, mat_d)?;
            moving = true;
        } else {
            moving |= step(&mut self.opacity, self.target_opacity, mat_r, mat_d, &mut self.cur_opacity)?;
        }

        if !self.matched_bud {
            moving |= step(
                &mut self.scale_progress,
                self.target_scale_progress,
                MATERIALIZE_SPRING[0],
                MATERIALIZE_SPRING[1],
                &mut self.cur_scale_progress,
            )?;
        }

        for i in 0..4 {
            moving |= step(&mut self.tint[i], self.target_tint[i], response, damping, &mut self.cur_tint[i])?;
        }

        if self.effects.advance(time)? {
            moving = true;
        }
        Ok(moving)
    }

    fn base_rect(&self) -> Rectangle {
        Rectangle {
            origin: self.cur_origin,
            size: [self.cur_size[0].max(1.0), self.cur_size[1].max(1.0)],
            radii: self.cur_radii.map(|r| r.max(0.0)),
            continuous: self.continuous,
        }
    }

    /// Applies `NativeGlassAppearance` `(1.2, 16, 16, 10)` vapor dispersion for unmatched
    /// entries, plus `ScalePulse` + `TranslationKick`.
    fn current_rect(&self) -> Rectangle {
        let mut rect = self.base_rect();
        if !self.matched_bud {
            let sp = self.cur_scale_progress.clamp(0.0, 1.5);
            if sp.abs() > 1.0e-5 {
                let dw = (0.2 * rect.size[0]).min(16.0) * sp;
                let dh = (0.2 * rect.size[1]).min(16.0) * sp;
                let new_w = (rect.size[0] + dw).max(1.0);
                let new_h = (rect.size[1] + dh).max(1.0);
                rect.origin[0] -= (new_w - rect.size[0]) * self.appearance_anchor[0];
                rect.origin[1] -= (new_h - rect.size[1]) * self.appearance_anchor[1];
                let r_scale = (new_w / rect.size[0]).min(new_h / rect.size[1]);
                rect.size = [new_w, new_h];
                rect.radii = rect.radii.map(|r| (r * r_scale).max(0.0));
            }
        }
        self.effects.apply(rect)
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

/// Authentic `26A434` material and shape matching container.
pub struct GlassContainer {
    entries: HashMap<u64, LiveEntry>,
    initialized: bool,
    adaptive_profile: AdaptiveProfile,
    max_match_distance: f64,
    pixel_length: f64,
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
            max_match_distance: DEFAULT_MAX_MATCH_DISTANCE,
            pixel_length: 1.0,
        }
    }

    /// Override the backdrop luminance adaptation profile (`26A434` V2 hysteresis, etc.).
    pub fn set_adaptive_profile(&mut self, profile: AdaptiveProfile) {
        self.adaptive_profile = profile;
    }

    /// Configure the maximum edge-gap distance (in points) for automatic `sourceElement` matching.
    pub fn set_max_match_distance(&mut self, max_distance: f64) -> Result<()> {
        if !max_distance.is_finite() || max_distance < 0.0 {
            return Err("finite nonnegative max_match_distance required".into());
        }
        self.max_match_distance = max_distance;
        Ok(())
    }

    /// Configure `FRINTA` pixel rounding length in points (`1.0` at `Scale::One`, `0.5` at `Scale::Two`).
    pub fn set_pixel_length(&mut self, pixel_length: f64) -> Result<()> {
        if !pixel_length.is_finite() || pixel_length <= 0.0 {
            return Err("positive finite pixel_length required".into());
        }
        self.pixel_length = pixel_length;
        Ok(())
    }

    /// Trigger an explicit `26A434` `ScalePulse` on an active entry.
    pub fn trigger_scale_pulse(&mut self, id: u64, anchor: [f64; 2], time: f64) -> Result<bool> {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.effects.trigger_pulse(anchor, 1.0, time)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Trigger an explicit `26A434` `TranslationKick` on an active entry.
    pub fn trigger_translation_kick(&mut self, id: u64, offset: [f64; 2], time: f64) -> Result<bool> {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.effects.trigger_kick(offset, 6.0, time)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Number of currently live (active + dematerializing) entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Submit the desired ordered slice of glass entries at `time` (in seconds).
    pub fn submit(&mut self, desired: &[GlassEntry], time: f64) -> Result<()> {
        if !time.is_finite() {
            return Err("finite submission time required".into());
        }
        let mut seen_ids = HashSet::with_capacity(desired.len());
        for entry in desired {
            entry.validate()?;
            if !seen_ids.insert(entry.id) {
                return Err(format!("duplicate glass entry id {}", entry.id));
            }
        }

        if !self.initialized {
            for (idx, entry) in desired.iter().enumerate() {
                self.entries.insert(entry.id, LiveEntry::spawn(entry, idx, time, false, None)?);
            }
            self.initialized = true;
            return Ok(());
        }

        // 1. Explicit `match_id` (`glassEffectID`) handoff from departing -> arriving entries.
        let mut handoff_by_match: HashMap<u64, LiveEntry> = HashMap::new();
        let departing_ids: Vec<u64> = self.entries.keys()
            .copied()
            .filter(|id| !seen_ids.contains(id))
            .collect();

        let new_match_ids: HashSet<u64> = desired.iter()
            .filter(|e| !self.entries.contains_key(&e.id))
            .filter_map(|e| e.match_id)
            .collect();

        let mut unmatched_departing = Vec::new();
        for id in departing_ids {
            let can_handoff = self.entries.get(&id)
                .and_then(|e| e.match_id)
                .is_some_and(|mid| new_match_ids.contains(&mid) && !handoff_by_match.contains_key(&mid));
            if can_handoff {
                let live = self.entries.remove(&id).unwrap();
                handoff_by_match.insert(live.match_id.unwrap(), live);
            } else {
                unmatched_departing.push(id);
            }
        }

        // Snapshot surviving candidates (ordered by previous order_index) for `sourceElement`.
        let mut surviving_candidates: Vec<(u64, RecipeRole, u64, i32, bool, Rectangle)> = self.entries
            .values()
            .filter(|e| seen_ids.contains(&e.id) && !e.departing)
            .map(|e| (e.id, e.role, e.group_id, e.z_index, e.allows_matching, e.current_rect()))
            .collect();
        surviving_candidates.sort_by_key(|&(id, ..)| self.entries[&id].order_index);

        // 2. Retarget surviving / `match_id`-inherited entries and collect newly arriving entries.
        let mut arriving_specs: Vec<(usize, &GlassEntry)> = Vec::new();
        for (idx, entry) in desired.iter().enumerate() {
            if let Some(live) = self.entries.get_mut(&entry.id) {
                live.retarget(entry, idx);
            } else if let Some(mut inherited) = entry.match_id.and_then(|mid| handoff_by_match.remove(&mid)) {
                inherited.retarget(entry, idx);
                self.entries.insert(entry.id, inherited);
            } else {
                arriving_specs.push((idx, entry));
            }
        }

        let all_desired_rects: Vec<(u64, RecipeRole, u64, Rectangle)> = desired.iter()
            .filter(|e| e.allows_matching)
            .map(|e| {
                let r = self.entries.get(&e.id).map_or(e.rect, LiveEntry::current_rect);
                (e.id, e.role, e.group_id, r)
            })
            .collect();

        let mut pulse_triggers: Vec<(u64, [f64; 2], Option<[f64; 2]>)> = Vec::new();
        for (idx, entry) in arriving_specs {
            let spatial = if entry.allows_matching && self.max_match_distance > 0.0 {
                select_source_element(
                    entry.id,
                    entry.role,
                    entry.group_id,
                    entry.rect,
                    &surviving_candidates,
                    &all_desired_rects,
                    self.max_match_distance,
                    self.pixel_length,
                )
            } else {
                None
            };
            if let Some((m, _)) = spatial {
                let partner_anchor = [1.0 - m.anchor[0], 1.0 - m.anchor[1]];
                pulse_triggers.push((m.candidate_id, partner_anchor, m.min_offset));
            }
            let spawned = LiveEntry::spawn(entry, idx, time, true, spatial)?;
            self.entries.insert(entry.id, spawned);
        }

        // 3. Process departing entries through `sourceElement` (`isRemoval = true`) so departing
        //    droplets retract smoothly back inside their partner surface!
        for id in unmatched_departing {
            let spatial = {
                let dep = &self.entries[&id];
                if !dep.departing && dep.allows_matching && self.max_match_distance > 0.0 {
                    select_source_element(
                        dep.id,
                        dep.role,
                        dep.group_id,
                        dep.current_rect(),
                        &surviving_candidates,
                        &all_desired_rects,
                        self.max_match_distance,
                        self.pixel_length,
                    )
                } else {
                    None
                }
            };
            if let Some((m, _)) = spatial {
                let partner_anchor = [1.0 - m.anchor[0], 1.0 - m.anchor[1]];
                let absorb_kick = m.min_offset.map(|[dx, dy]| [-dx, -dy]);
                pulse_triggers.push((m.candidate_id, partner_anchor, absorb_kick));
            }
            if let Some(live) = self.entries.get_mut(&id) {
                live.mark_departing(spatial, time)?;
            }
        }

        for (_, mut leftover) in handoff_by_match {
            leftover.mark_departing(None, time)?;
            self.entries.insert(leftover.id, leftover);
        }

        for (partner_id, anchor, kick) in pulse_triggers {
            if let Some(partner) = self.entries.get_mut(&partner_id) {
                partner.effects.trigger_pulse(anchor, 0.35, time)?;
                if let Some(off) = kick {
                    partner.effects.trigger_kick(off, 4.0, time)?;
                }
            }
        }

        Ok(())
    }

    /// Advance all geometry, appearance, pulse, and kick springs to `time`.
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

    /// Resolve `26A434` backdrop luminance adaptation, conditional Z-sort (`0x185deb67c`),
    /// and contiguous `DisplayMaterial` chunking (`0x185ded578`).
    pub fn resolve(&mut self, source: &Frame) -> Result<Vec<ResolvedGroup>> {
        let viewport = source.viewport();
        self.pixel_length = 1.0 / f64::from(viewport.scale().factor());

        let mut ids: Vec<u64> = self.entries.keys().copied().collect();
        ids.sort_by_key(|id| {
            let e = &self.entries[id];
            (e.order_index, e.id)
        });

        struct Candidate {
            resolved: ResolvedEntry,
            order_index: usize,
        }

        let mut items = Vec::with_capacity(ids.len());
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
                let sel = entry.backdrop.update(&probe_recipe, self.adaptive_profile, 0, source, obs)?;
                let desired_dark = if matches!(sel.appearance, Appearance::Dark) { 1.0 } else { 0.0 };
                entry.target_dark_mix = desired_dark;
                if let Some(luma) = sel.luminance {
                    entry.target_luma = Some(f64::from(luma));
                    if entry.luma_spring.is_none() {
                        entry.luma_spring = Some(Spring::new(f64::from(luma), entry.last_advance_time)?);
                        entry.cur_luma = Some(luma);
                    }
                } else {
                    entry.target_luma = None;
                    entry.luma_spring = None;
                    entry.cur_luma = None;
                }
                if !entry.backdrop_initialized {
                    entry.cur_dark_mix = desired_dark;
                    entry.dark_mix = Spring::new(desired_dark, entry.last_advance_time)?;
                    entry.backdrop_initialized = true;
                }
                sel
            } else {
                Selection { active: false, appearance: entry.appearance, luminance: None }
            };

            let dark_mix = (entry.cur_dark_mix as f32).clamp(0.0, 1.0);
            let effective_appearance = if dark_mix >= 0.5 {
                Appearance::Dark
            } else {
                Appearance::Light
            };
            let smoothed_selection = Selection {
                active: selection.active,
                appearance: effective_appearance,
                luminance: entry.cur_luma.or(selection.luminance),
            };

            items.push(Candidate {
                resolved: ResolvedEntry {
                    id: entry.id,
                    match_id: entry.match_id,
                    matched_partner_id: entry.matched_partner_id,
                    group_id: entry.group_id,
                    z_index: entry.z_index,
                    rect,
                    role: entry.role,
                    configured_appearance: entry.appearance,
                    appearance: effective_appearance,
                    dark_mix,
                    selection: smoothed_selection,
                    diffusion,
                    tint,
                    opacity,
                    smoothness: entry.smoothness,
                },
                order_index: entry.order_index,
            });
        }

        if items.is_empty() {
            return Ok(Vec::new());
        }

        // `26A434` AppKit conditional Z-sort (`0x185deb67c`):
        // Skip sorting when all entries share one `DisplayMaterial` identity; otherwise
        // run stable Z-sort and split into contiguous chunks (`0x185ded578`).
        let all_same_identity = items.windows(2).all(|w| same_material_identity(&w[0].resolved, &w[1].resolved));
        if !all_same_identity {
            items.sort_by_key(|c| (c.resolved.z_index, c.order_index, c.resolved.id));
        }

        let mut chunks: Vec<Vec<ResolvedEntry>> = Vec::new();
        for cand in items {
            let append_existing = chunks.last().is_some_and(|chunk| {
                same_material_identity(&chunk[0], &cand.resolved)
            });
            if append_existing {
                chunks.last_mut().unwrap().push(cand.resolved);
            } else {
                chunks.push(vec![cand.resolved]);
            }
        }

        let mut out = Vec::with_capacity(chunks.len());
        for entries in chunks {
            let first = entries[0];
            let smoothness = if entries.len() > 1 {
                entries.iter().map(|e| e.smoothness).fold(0.0_f32, f32::max)
            } else {
                0.0
            };
            let avg_diffusion = entries.iter().map(|e| e.diffusion).sum::<f32>() / entries.len() as f32;
            let group_opacity = if entries.len() == 1 {
                first.opacity
            } else {
                entries.iter().map(|e| e.opacity).fold(0.0_f32, f32::max)
            };
            let dims = dimensions_for_group(&entries);
            // Area-weighted luminance & dark_mix across shared DisplayMaterial members so
            // grouped surfaces share one smooth material state without step-flashes.
            let mut total_area = 0.0_f32;
            let mut weighted_dark = 0.0_f32;
            let mut luma_area = 0.0_f32;
            let mut weighted_luma = 0.0_f32;
            for e in &entries {
                let area = ((e.rect.size[0] * e.rect.size[1]).max(1.0)) as f32;
                total_area += area;
                weighted_dark += area * e.dark_mix;
                if let Some(l) = e.selection.luminance {
                    luma_area += area;
                    weighted_luma += area * l;
                }
            }
            let avg_dark = if total_area > 0.0 {
                (weighted_dark / total_area).clamp(0.0, 1.0)
            } else {
                first.dark_mix
            };
            let group_luma = (luma_area > 0.0).then_some(weighted_luma / luma_area);

            let multi_tinted = entries.len() > 1 && entries.iter().any(|e| e.tint.is_some());
            let recipe_tint = if !multi_tinted { first.tint } else { None };
            let material = if avg_dark <= 0.002 {
                SdrRecipe {
                    role: first.role,
                    appearance: Appearance::Light,
                    dimensions: dims,
                    diffusion: avg_diffusion,
                    tint: recipe_tint,
                    style: BackgroundStyle::default(),
                }
                .build_with_luminance(viewport, group_luma)?
            } else if avg_dark >= 0.998 {
                SdrRecipe {
                    role: first.role,
                    appearance: Appearance::Dark,
                    dimensions: dims,
                    diffusion: avg_diffusion,
                    tint: recipe_tint,
                    style: BackgroundStyle::default(),
                }
                .build_with_luminance(viewport, group_luma)?
            } else {
                let light_mat = SdrRecipe {
                    role: first.role,
                    appearance: Appearance::Light,
                    dimensions: dims,
                    diffusion: avg_diffusion,
                    tint: recipe_tint,
                    style: BackgroundStyle::default(),
                }
                .build_with_luminance(viewport, group_luma)?;
                let dark_mat = SdrRecipe {
                    role: first.role,
                    appearance: Appearance::Dark,
                    dimensions: dims,
                    diffusion: avg_diffusion,
                    tint: recipe_tint,
                    style: BackgroundStyle::default(),
                }
                .build_with_luminance(viewport, group_luma)?;
                light_mat.lerp(&dark_mat, avg_dark)?
            };

            let mut shape_tags = Vec::new();
            let mut tints = Vec::new();
            if multi_tinted {
                for (idx, item) in entries.iter().enumerate() {
                    let tag = (idx as u64) + 1;
                    shape_tags.push(tag);
                    if let Some(rgba) = item.tint {
                        let a = rgba[3] * item.opacity;
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
                group_id: first.group_id,
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

    /// Advance springs to `time`, resolve adapted materials and contiguous `DisplayMaterial`
    /// groups, and composite onto a new `Frame`.
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

/// Places `own` tucked smoothly inside `partner`'s facing rim (`matchedGeometry` source frame)
/// so emerging/retracting droplets bulge organically through `partner`'s wall via `sdf-union.frag`.
fn tucked_rect_inside_partner(partner: Rectangle, own: Rectangle) -> Rectangle {
    let w = (own.size[0] * 0.82).min(partner.size[0] - 4.0).max(4.0);
    let h = (own.size[1] * 0.82).min(partner.size[1] - 4.0).max(4.0);
    let ocx = own.origin[0] + own.size[0] * 0.5;
    let ocy = own.origin[1] + own.size[1] * 0.5;
    let x = (ocx - w * 0.5).clamp(
        partner.origin[0] + 2.0,
        (partner.origin[0] + partner.size[0] - w - 2.0).max(partner.origin[0] + 2.0),
    );
    let y = (ocy - h * 0.5).clamp(
        partner.origin[1] + 2.0,
        (partner.origin[1] + partner.size[1] - h - 2.0).max(partner.origin[1] + 2.0),
    );
    let r_scale = (w / own.size[0]).min(h / own.size[1]);
    Rectangle {
        origin: [x, y],
        size: [w, h],
        radii: own.radii.map(|r| (r * r_scale).max(2.0)),
        continuous: own.continuous,
    }
}

/// `26A434` `AppKit` / `SwiftUI` `DisplayMaterial.ID` chunk equality (`0x185df53c4`, `0x224ba86b8`),
/// plus an **extension beyond Apple's native container model**:
/// In Apple's `GlassEffectContainer`, all members share one `BackdropState` from the start
/// (separate adaptive surfaces live in separate containers and never `sdfUnion`-merge).
/// Here, individual entries with the same `group_id` may adapt independently (`Light` vs `Dark`)
/// in separate passes and dynamically merge into one `sdfUnion` group once their `dark_mix`
/// converges within `<= 0.005` (`< 1/255` sRGB code value, eliminating any join flash).
fn same_material_identity(a: &ResolvedEntry, b: &ResolvedEntry) -> bool {
    a.group_id == b.group_id
        && a.role == b.role
        && a.appearance == b.appearance
        && (a.dark_mix - b.dark_mix).abs() <= 0.005
        && (a.diffusion - b.diffusion).abs() <= 0.05
        && (a.smoothness - b.smoothness).abs() <= 0.05
}

/// Exact `26A434` `CGRect.minDistance` (`0x2252722b8`) with `FRINTA` ties-to-even pixel rounding
/// and `Entry.geometryResult` (`0x22487fff0`) directed factor-3 size gate.
pub fn evaluate_geometry_match(
    target_rect: Rectangle,
    candidate_id: u64,
    candidate_rect: Rectangle,
    pixel_length: f64,
) -> Option<GeometryMatch> {
    let tw = target_rect.size[0].abs();
    let th = target_rect.size[1].abs();
    let cw = candidate_rect.size[0].abs();
    let ch = candidate_rect.size[1].abs();
    if tw <= 0.0 || th <= 0.0 || cw <= 0.0 || ch <= 0.0 {
        return None;
    }

    let tx0 = target_rect.origin[0];
    let ty0 = target_rect.origin[1];
    let tx1 = tx0 + tw;
    let ty1 = ty0 + th;

    let cx0 = candidate_rect.origin[0];
    let cy0 = candidate_rect.origin[1];
    let cx1 = cx0 + cw;
    let cy1 = cy0 + ch;

    let gap_x = (tx0 - cx1).max(cx0 - tx1);
    let gap_y = (ty0 - cy1).max(cy0 - ty1);
    let overlapping = gap_x < 0.0 && gap_y < 0.0;

    if !overlapping && (tw >= 3.0 * cw || th >= 3.0 * ch) {
        return None;
    }

    let round_px = |v: f64| -> f64 {
        if v <= 0.0 {
            0.0
        } else {
            (v / pixel_length).round_ties_even() * pixel_length
        }
    };
    let rx = round_px(gap_x);
    let ry = round_px(gap_y);
    let distance = rx.hypot(ry);

    let tcx = tx0 + tw * 0.5;
    let tcy = ty0 + th * 0.5;
    let ccx = cx0 + cw * 0.5;
    let ccy = cy0 + ch * 0.5;

    let min_offset = if overlapping {
        None
    } else {
        Some([
            if gap_x > 0.0 { ccx - tcx } else { 0.0 },
            if gap_y > 0.0 { ccy - tcy } else { 0.0 },
        ])
    };

    let anchor_axis = |tc: f64, cc: f64, span: f64| -> f64 {
        let diff = cc - tc;
        if diff.abs() <= span * 0.25 {
            0.5
        } else if diff < 0.0 {
            0.0
        } else {
            1.0
        }
    };
    let anchor = [anchor_axis(tcx, ccx, tw), anchor_axis(tcy, ccy, th)];

    Some(GeometryMatch {
        candidate_id,
        distance,
        overlapping,
        min_offset,
        anchor,
    })
}

/// Exact `26A434` `sourceElement` (`0x22453991c`) with `hasCloseElement` (`0x22453fda4`)
/// stepping-stone bridging and smallest-distance selection (`canOverride` `0x22453fb88`).
#[allow(clippy::too_many_arguments)]
fn select_source_element(
    target_id: u64,
    target_role: RecipeRole,
    target_group: u64,
    target_rect: Rectangle,
    candidates: &[(u64, RecipeRole, u64, i32, bool, Rectangle)],
    other_elements: &[(u64, RecipeRole, u64, Rectangle)],
    max_distance: f64,
    pixel_length: f64,
) -> Option<(GeometryMatch, Rectangle)> {
    let mut best: Option<(GeometryMatch, Rectangle)> = None;

    for &(cid, crole, cgroup, _cz, callows, crect) in candidates {
        if cid == target_id || !callows || crole != target_role || cgroup != target_group {
            continue;
        }
        let Some(m) = evaluate_geometry_match(target_rect, cid, crect, pixel_length) else {
            continue;
        };

        let within_limit = if m.distance <= max_distance {
            true
        } else {
            other_elements.iter().any(|&(oid, orole, ogroup, orect)| {
                oid != target_id
                    && oid != cid
                    && orole == target_role
                    && ogroup == target_group
                    && evaluate_geometry_match(target_rect, oid, orect, pixel_length)
                        .is_some_and(|m1| m1.distance <= max_distance)
                    && evaluate_geometry_match(orect, cid, crect, pixel_length)
                        .is_some_and(|m2| m2.distance <= max_distance)
            })
        };

        if !within_limit {
            continue;
        }

        let overrides = match &best {
            None => true,
            Some((prev, _)) => m.distance + 1.0e-9 < prev.distance,
        };
        if overrides {
            best = Some((m, crect));
        }
    }

    best
}

fn dimensions_for_rect(rect: Rectangle) -> Dimensions {
    let min_side = rect.size[0].min(rect.size[1]);
    Dimensions {
        minimum: Some(min_side),
        largest_area_minimum: Some(min_side),
    }
}

/// Exact `26A434` `PathSet.shapeMetrics` (`context_shape_metrics`):
/// `minimum` is `min_i(min(w_i, h_i))` and `largest_area_minimum` is `min(w, h)` of the
/// single largest-area shape — completely independent of `(x, y)` separation so moving
/// one shape away never alters another shape's blur sharpness or refraction depth!
fn dimensions_for_group(entries: &[ResolvedEntry]) -> Dimensions {
    let mut minimum = f64::INFINITY;
    let mut largest_area = -1.0_f64;
    let mut largest_area_min = 48.0_f64;
    for e in entries {
        let w = e.rect.size[0].abs();
        let h = e.rect.size[1].abs();
        let min_side = w.min(h);
        minimum = minimum.min(min_side);
        let area = w * h;
        if area > largest_area {
            largest_area = area;
            largest_area_min = min_side;
        }
    }
    Dimensions {
        minimum: Some(minimum.max(1.0)),
        largest_area_minimum: Some(largest_area_min.max(1.0)),
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
