//! SDR backdrop appearance for supplied raster footprints and the 26A434 V2 profile.
use super::{
    pack::binary16_bits,
    recipe::{PreparedSdrMaterial, RecipeRole, SdrRecipe},
};
use crate::{
    Result,
    frame::Frame,
    overlay::{Appearance, decode},
    raster::PixelRect,
};

const ADAPTIVE: u64 = 0x400;
const DISABLE_ADAPTIVE: u64 = 0x80;
const PRESERVE_LUMINANCE: u64 = 0x10000;

#[derive(Clone, Copy)]
pub struct Hysteresis {
    pub light: [f32; 2],
    pub dark: [f32; 2],
}

impl Default for Hysteresis {
    fn default() -> Self {
        Self { light: [0.1, 0.3], dark: [0.7, 0.9] }
    }
}

impl Hysteresis {
    fn select(self, environment: Appearance, previous: Appearance, luminance: f32) -> Appearance {
        let [low, high] = match environment {
            Appearance::Light => self.light,
            Appearance::Dark => self.dark,
        };
        if luminance < low {
            Appearance::Dark
        } else if luminance > high {
            Appearance::Light
        } else {
            previous
        }
    }
}

/// Profile inputs for the V2 SDR recipes. The default is the retail fallback.
#[derive(Clone, Copy, Default)]
pub struct Profile {
    pub adaptive_v2_enabled: bool,
    pub background_content_word: u64,
    pub reduce_transparency: bool,
    /// Raw native VariantOptions overrides. Additions take precedence.
    pub options_added: u64,
    pub options_removed: u64,
    pub fixed_luminance: Option<f32>,
    pub initial_luminance: Option<f32>,
    pub explicit_appearance: Option<Appearance>,
    pub hysteresis: Hysteresis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub active: bool,
    pub appearance: Appearance,
    pub luminance: Option<f32>,
}

/// One state per glass surface. Drop the state when the surface is removed.
#[derive(Clone, Copy)]
pub struct BackdropState {
    luminance: Option<f32>,
    adapted: Option<Appearance>,
    awaiting_initial: bool,
}

impl Default for BackdropState {
    fn default() -> Self {
        Self { luminance: None, adapted: None, awaiting_initial: true }
    }
}

impl BackdropState {
    /// Measure the backdrop, select appearance, and build the adapted SDR material.
    /// An error leaves the previous state unchanged.
    pub fn prepare(
        &mut self,
        mut recipe: SdrRecipe,
        profile: Profile,
        surface_size: u8,
        source: &Frame,
        observation: Option<PixelRect>,
    ) -> Result<(Selection, PreparedSdrMaterial)> {
        let mut next = *self;
        let selection = next.update(&recipe, profile, surface_size, source, observation)?;
        recipe.appearance = selection.appearance;
        let material = recipe.build_with_luminance(source.viewport(), selection.luminance)?;
        *self = next;
        Ok((selection, material))
    }

    /// Update the backdrop state from the current SDR frame.
    ///
    /// The caller supplies the source frame and observation rectangle in pixels
    /// before the glass pass. None represents an unavailable observation. The
    /// native geometry gate uses the supplied largest-area minimum in points.
    /// Surface size uses the original byte: 2 and 3 block measurement.
    pub fn update(
        &mut self,
        recipe: &SdrRecipe,
        profile: Profile,
        surface_size: u8,
        source: &Frame,
        observation: Option<PixelRect>,
    ) -> Result<Selection> {
        if [profile.fixed_luminance, profile.initial_luminance].into_iter().flatten().any(|v| !v.is_finite())
            || recipe.dimensions.largest_area_minimum.is_some_and(|v| !v.is_finite())
            || [profile.hysteresis.light, profile.hysteresis.dark].iter().any(|[lo, hi]| {
                !lo.is_finite() || !hi.is_finite() || lo > hi
            })
        {
            return Err("invalid backdrop appearance input".into());
        }

        let base_options = match recipe.role {
            RecipeRole::Regular => ADAPTIVE,
            RecipeRole::Clear => 0,
            RecipeRole::Dock => 1,
            RecipeRole::Popover => return Err("popover uses an explicit appearance".into()),
        };
        let options = (base_options & !profile.options_removed) | profile.options_added;
        let requested = options & ADAPTIVE != 0 && options & DISABLE_ADAPTIVE == 0
            && !profile.reduce_transparency;
        let allowed = profile.fixed_luminance.is_some()
            || (profile.adaptive_v2_enabled && profile.background_content_word & !2 == 0);
        let active = requested && allowed
            && recipe.dimensions.largest_area_minimum.is_none_or(|v| v <= 64.0)
            && !matches!(surface_size, 2 | 3);

        let mut next = *self;
        let fallback = profile.explicit_appearance.unwrap_or(recipe.appearance);
        let appearance = if active {
            let measured = observation.map(|rect| measure(source, rect)).transpose()?.flatten();
            next.luminance = profile.fixed_luminance.or(measured)
                .or(profile.initial_luminance.filter(|_| self.awaiting_initial))
                .or(self.luminance.filter(|_| options & PRESERVE_LUMINANCE != 0));
            next.awaiting_initial = self.awaiting_initial && measured.is_none();
            let selected = profile.explicit_appearance.unwrap_or_else(|| {
                next.luminance.map_or(recipe.appearance, |luma| {
                    profile.hysteresis.select(recipe.appearance, self.adapted.unwrap_or(recipe.appearance), luma)
                })
            });
            next.adapted = Some(selected);
            selected
        } else {
            next.luminance = None;
            next.adapted = None;
            fallback
        };

        *self = next;
        Ok(Selection { active, appearance, luminance: next.luminance })
    }
}

/// Measure a supplied SDR pixel rectangle using half-precision linear texels.
/// The accumulation is the host's row-major f32 reduction.
pub fn measure(frame: &Frame, rect: PixelRect) -> Result<Option<f32>> {
    let size = frame.viewport().pixels();
    rect.check(size)?;
    let mut sum = [0.0f32; 4];
    for row in rect.row_spans(size) {
        for &pixel in &frame.pixels()[row.start / 4..row.end / 4] {
            for (total, value) in sum.iter_mut().zip(decode(pixel)) {
                if value.abs() > 65_504.0 {
                    return Err("backdrop exceeds binary16 range".into());
                }
                let half = binary16_bits(value)?;
                let magnitude = half & 0x7fff;
                let value = if magnitude < 0x400 {
                    f32::from(magnitude) * (1.0 / 16_777_216.0)
                } else {
                    f32::from_bits((u32::from(magnitude) << 13) + (112 << 23))
                };
                *total += if half & 0x8000 == 0 { value } else { -value };
            }
        }
    }
    let [r, g, b, alpha] = sum.map(f64::from);
    if alpha <= 0.0 {
        return Ok(None);
    }
    let value = ((g.mul_add(0.7152, r * 0.2126) + b * 0.0722) / alpha) as f32;
    let quantized = (value * 64.0).round() / 64.0;
    Ok((quantized >= 0.0).then_some(quantized))
}
