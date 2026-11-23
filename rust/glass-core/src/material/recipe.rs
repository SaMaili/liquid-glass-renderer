//! Active SDR Dock, Regular and Clear recipes for the agreed renderer scenes.
use super::{
    spec::{BlurDistance, BlurSpec, BoundedDistance, Dimensions, EdgeBleedSpec, MaterialSpec,
           ParameterScale, RefractionSpec, ShadowSpec},
    BackgroundMaterial, BackgroundParameters, BackgroundStyle, BlurFill, ColorMatrices,
    Effects, Geometry, Highlights, YccColor, apply_dodge_fill, linear_fill_matrix,
};
use crate::{Result, Size, overlay::Appearance, raster::Viewport, scene::blur_level_count};

/// Exact 26A434 CASDFKeyFillHighlightEffect (Parameters.highlights) payload for
/// the overlaid SDFLayer.GroupLayer pass (sdf_diffuse_kf_highlight +
/// backdrop_aware_vibrant_color_matrix_sover).
#[derive(Clone, Copy)]
pub struct KeyFillHighlight {
    /// RGB rows of the 4×5 highlight YCC (+ optional dodgeFill) matrix packed as
    /// [m0, m1, m2, m4] per row, followed by [keyDir.x, keyDir.y, clampMax, opacity].
    pub matrix: [[f32; 4]; 4],
    /// [0] = [sharpHeight, cos(sharpSpread), 1/sharpAmount - 2, curvature]
    /// [1] = [diffuseHeight, cos(diffuseSpread), 1/diffuseAmount - 2, inset]
    pub lobes: [[f32; 4]; 2],
}

impl KeyFillHighlight {
    /// Exact 26A434 Parameters.highlights resolved for macOS retail V2 (keyAngle = 0).
    pub fn for_recipe(role: RecipeRole, appearance: Appearance) -> Result<Self> {
        let dark = matches!(appearance, Appearance::Dark);
        let (ycc, dodge_fill) = match (role, dark) {
            (RecipeRole::Dock, false) => (
                YccColor { black: 0.15, white: 0.9, saturation: 1.0, fill: [0.0; 4] },
                Some([0.05087609_f32, 0.05087609, 0.05087609, 1.0]),
            ),
            (RecipeRole::Dock, true) => (
                YccColor { black: 0.25, white: 0.95, saturation: 1.2, fill: [0.0; 4] },
                None,
            ),
            (RecipeRole::Regular | RecipeRole::Popover, _) => (
                YccColor { black: 0.125, white: 0.9, saturation: 1.0, fill: [0.0; 4] },
                Some([0.01960665_f32, 0.01960665, 0.01960665, 1.0]),
            ),
            (RecipeRole::Clear, _) => (
                YccColor { black: 0.15, white: 1.1, saturation: 1.0, fill: [0.0; 4] },
                Some([0.05087609_f32, 0.05087609, 0.05087609, 1.0]),
            ),
        };
        let mut mat = ycc.matrix()?;
        if let Some(dodge) = dodge_fill {
            mat = apply_dodge_fill(mat, dodge);
        }
        // 26A434 key & fill parameters: height = 1.0 pt, spread = 80°, curvature = 0.75,
        // amount = 0.5, diffuse = (heightScale: 8.0, spreadScale: 0.65, amountScale: 0.15),
        // keyAngle = 0.0 -> keyDir = (sin(0), -cos(0)) = (0, -1), fillDir = (0, +1).
        let spread: f64 = 1.3962634015954636; // 80°
        let sharp_amount: f64 = 0.5;
        let diffuse_spread = spread * 0.65; // 52°
        let diffuse_amount = sharp_amount * 0.15; // 0.075
        Ok(Self {
            matrix: [
                [mat[0][0], mat[0][1], mat[0][2], mat[0][4]],
                [mat[1][0], mat[1][1], mat[1][2], mat[1][4]],
                [mat[2][0], mat[2][1], mat[2][2], mat[2][4]],
                [0.0, -1.0, 1.0, 1.0],
            ],
            lobes: [
                [1.0, spread.cos() as f32, (1.0 / sharp_amount - 2.0) as f32, 0.75],
                [8.0, diffuse_spread.cos() as f32, (1.0 / diffuse_amount - 2.0) as f32, 0.0],
            ],
        })
    }
}

#[derive(Clone, Copy)]
pub enum RecipeRole {
    Dock,
    Regular,
    Clear,
    Popover,
}

/// Inputs for the demo's active SDR material context (26A434, retail V2).
/// Geometry lives in points. Tint is straight encoded sRGB.
pub struct SdrRecipe {
    pub role: RecipeRole,
    pub appearance: Appearance,
    pub dimensions: Dimensions,
    pub diffusion: f32,
    pub tint: Option<[f32; 4]>,
    pub style: BackgroundStyle,
}

pub struct PreparedSdrMaterial {
    pub material: BackgroundMaterial,
    pub blur_levels: u32,
}

pub struct LayerEffects {
    pub specular: bool,
    pub highlight_offset: Option<f64>,
    pub shadow_opacity: Option<f32>,
    pub blur_radius: Option<f32>,
    pub refraction_depth: Option<f64>,
}

impl Default for LayerEffects {
    fn default() -> Self {
        Self {
            specular: true,
            highlight_offset: None,
            shadow_opacity: None,
            blur_radius: None,
            refraction_depth: None,
        }
    }
}

impl SdrRecipe {
    pub fn build(self, viewport: Viewport) -> Result<PreparedSdrMaterial> {
        self.build_with_luminance(viewport, None)
    }

    /// Prepare the SDR recipe using an already selected appearance and observation.
    pub fn build_with_luminance(
        self,
        viewport: Viewport,
        luminance: Option<f32>,
    ) -> Result<PreparedSdrMaterial> {
        self.build_for_raster(viewport.pixels(), viewport.scale().factor() as f32, luminance)
    }

    /// Resolve the same material for a compositor-owned raster at its actual
    /// pixels-per-point scale. The supplied raster may include filter padding.
    pub fn build_for_raster(
        self,
        pixels: Size,
        scale: f32,
        luminance: Option<f32>,
    ) -> Result<PreparedSdrMaterial> {
        self.build_with_layer_effects(pixels, scale, luminance, LayerEffects::default())
    }

    /// Override the recipe's highlights and shadow for an authored layer.
    pub fn build_with_layer_effects(
        self,
        pixels: Size,
        scale: f32,
        luminance: Option<f32>,
        layer: LayerEffects,
    ) -> Result<PreparedSdrMaterial> {
        pixels.validate()?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err("positive finite material raster scale required".into());
        }
        if luminance.is_some_and(|value| !value.is_finite()) {
            return Err("finite recipe luminance required".into());
        }
        if !(0.0..=1.0).contains(&self.diffusion)
            || self.tint.is_some_and(|values| !values.iter().all(|v| (0.0..=1.0).contains(v)))
        {
            return Err("recipe diffusion and tint must be in [0, 1]".into());
        }

        let dock = matches!(self.role, RecipeRole::Dock);
        let clear = matches!(self.role, RecipeRole::Clear);
        let popover = matches!(self.role, RecipeRole::Popover);
        let regular = matches!(self.role, RecipeRole::Regular | RecipeRole::Popover);
        let dark = matches!(self.appearance, Appearance::Dark);
        let clamped = |output| ParameterScale::ClampedLinear {
            input: [64.0, 160.0], output, upper_tail: None,
        };
        let bound = |normalized, bounds| BoundedDistance { normalized, bounds };
        let unbounded = [f64::NEG_INFINITY, f64::INFINITY];
        let [inner_height, inner_amount, outer_height, outer_amount] = match self.role {
            RecipeRole::Dock => [0.08, -0.18, 0.13, 0.42],
            RecipeRole::Regular | RecipeRole::Popover => [0.25, -0.5, 0.2, 0.25],
            RecipeRole::Clear => [0.36, -0.65, 0.125, 0.2],
        };
        let mut spec = MaterialSpec {
            blur: BlurSpec {
                opacity: 1.0, opacities: [1.0, 0.5, 0.5, 1.0],
                distances: [BlurDistance::Inset(0.5), BlurDistance::Fixed(-1.0),
                            BlurDistance::Fixed(0.0), BlurDistance::Fixed(0.0)],
                radius: match self.role {
                    RecipeRole::Dock => ParameterScale::Constant(1.5),
                    RecipeRole::Regular | RecipeRole::Popover => clamped([0.0, 4.0]),
                    RecipeRole::Clear => ParameterScale::Constant(0.75),
                },
                outer_refraction_blur_radius: regular.then_some(5.0),
            },
            refraction: RefractionSpec {
                inner_height: bound(inner_height, [0.0,20.0]),
                inner_amount: bound(inner_amount, [-60.0,f64::INFINITY]),
                outer_height: bound(outer_height, if regular { [16.0,f64::INFINITY] } else { unbounded }),
                outer_amount: bound(outer_amount, if regular { [16.0,f64::INFINITY] } else { unbounded }),
                outer_opacity: if regular { 0.6 } else if dock { 0.2 } else { 0.0 },
            },
            edge_bleed: EdgeBleedSpec {
                opacity: if regular { clamped([0.0, if dark { 0.8 } else { 0.5 }]) }
                          else { ParameterScale::Constant(0.0) },
                normalized_height: 0.35, normalized_amount: 0.35, normalized_blur_radius: 0.35,
                max_blur_radius: if regular { 160.0 } else { 120.0 },
                // The demo supplies its face matrix for EdgeBleed. These bytes
                // are carried only by the Spec snapshot interface.
                ycc: [0; 69], use_darken_blending: !(regular && dark),
            },
            shadow: ShadowSpec {
                normalized_height: if regular { 0.0 } else { 0.4 },
                normalized_amount: if regular { 0.0 } else { 0.625 },
                blur_radius: if regular { 0.0 } else { 40.0 },
                opacity: if regular { clamped([0.04, if dark { 0.6 } else { 0.4 }]) }
                          else { ParameterScale::Constant(0.0) },
                radius: if regular { ParameterScale::ClampedLinear {
                    input: [64.0,160.0], output: [4.0,24.0], upper_tail: None,
                } } else { ParameterScale::Constant(24.0) },
                offset: [0.0,8.0], ycc: [0;69], vibrancy_contribution: ParameterScale::Constant(0.0),
            },
            fill: BlurFill { radius: 0.0, lighten: 0.0, darken: 0.0, normal: 0.0 },
        };

        let mut face = if dock && dark {
            YccColor { white: 0.92, black: 0.05, saturation: 1.25, fill: [0.0,0.0,0.0,0.02] }
        } else if dock {
            YccColor { white: 0.9, black: 0.15, saturation: 1.25, fill: [1.0,1.0,1.0,0.0] }
        } else if dark {
            YccColor { white: 1.125, black: 0.125, saturation: 1.3, fill: [0.0;4] }
        } else {
            YccColor { white: 1.03, black: 0.4, saturation: 1.2, fill: [1.0,1.0,1.0,0.2] }
        };
        let mut face_max_luma = if dock && dark { 0.75 } else if popover && !dark { 0.94 } else { 1.0 };

        if regular {
            spec.fill = if dark { BlurFill { radius: 16.0, lighten: 0.0, darken: 0.9, normal: 0.5 } }
                else { BlurFill { radius: 16.0, lighten: 0.9, darken: 0.0, normal: 0.5 } };
            if dark {
                face_max_luma = regular_dark_luma(self.dimensions,0.35)?;
            }
            regular_diffusion(&mut spec.fill,&mut face,&mut face_max_luma,self.diffusion,self.dimensions,dark)?;
        } else {
            spec.blur.apply_clear_diffusion(Some(self.diffusion),true,&mut spec.fill)?;
        }

        if regular && let Some(luminance) = luminance {
            // 26A434 adaptivity at 0x23f537c4c..0x23f537c9c.
            // Regular supplies -1/+1 offsets and fixed alpha for this recipe.
            let luminance = luminance.min(1.0);
            let [black, white, alpha]: [f32; 3] = if dark {
                [0.05, 0.9, 0.075]
            } else {
                [0.4, 1.03, 0.2]
            };
            face.black = black.max(luminance - 1.0);
            face.white = white.min(luminance + 1.0);
            face.fill[3] = alpha;
        }

        let mut resolved = spec.resolve(self.dimensions,u64::MAX,1.0,true)?;
        if let Some(opacity) = layer.shadow_opacity {
            if !(0.0..=1.0).contains(&opacity) {
                return Err("layer shadow opacity must be in [0, 1]".into());
            }
            resolved.shadow.opacity = opacity;
        }
        if let Some(radius) = layer.blur_radius {
            if !radius.is_finite() || radius < 0.0 {
                return Err("nonnegative finite layer blur radius required".into());
            }
            resolved.blur.radius = radius;
        }
        if let Some(depth) = layer.refraction_depth {
            if !(0.0..=1.0).contains(&depth) {
                return Err("layer refraction depth must be in [0, 1]".into());
            }
            resolved.refraction.inner_height *= depth * 2.0;
            resolved.refraction.outer_height *= depth * 2.0;
        }
        if popover {
            // Executed CartouchePopover's spec modifier and its following
            // legacy pass leave the inner height at 5 and resolve the outer
            // band from the unmodified, dimension-dependent amount.
            resolved.refraction.inner_height = 5.0;
        }
        resolved.canonicalize_inactive_radii();
        let fill_radius = resolved.fill.as_ref().map_or(0.0, |fill| fill.radius);
        let radius = resolved.blur.radius.max(fill_radius) * 0.5 * scale;
        let blur_levels = blur_level_count(pixels,radius)?;

        let tint = if dock { None } else { self.tint.map(|[r,g,b,a]| {
            [linear_srgb(r)*a,linear_srgb(g)*a,linear_srgb(b)*a,a]
        }) };
        let face_matrix = if clear {
            linear_fill_matrix(tint.unwrap_or([0.0;4]))
        } else {
            let alpha = face.fill[3];
            for v in &mut face.fill[..3] { *v *= alpha; }
            if let Some(tint) = tint {
                face.fill = std::array::from_fn(|i| {
                    (f64::from(face.fill[i])*(1.0-f64::from(tint[3]))+f64::from(tint[i])) as f32
                });
            }
            face.matrix()?
        };
        let shadow_matrix = YccColor {
            white: 1.0, black: 0.0, saturation: if regular { 1.0 } else { 1.2 },
            fill: if popover { [0.0, 0.0, 0.0, 0.3] } else { [0.0;4] },
        }.matrix()?;
        let bleed_matrix = if popover {
            let (black, white, saturation) = if dark { (0.125, 0.5, 1.0) } else { (0.9, 1.0, 1.2) };
            YccColor { white, black, saturation, fill: [0.0; 4] }.matrix()?
        } else {
            face_matrix
        };
        let geometry = Geometry {
            transform: [1.0,0.0,0.0,1.0], source_size: [pixels.width,pixels.height],
            target_size: [pixels.width,pixels.height], source_scale: scale, output_scale: scale,
            sdf_transform: None,
        };
        let (dark_spread, dark_spread_sdr) = match (self.role, dark) {
            (RecipeRole::Dock, _) => (2.0943951023931953, 1.3089969389957472),
            (RecipeRole::Regular | RecipeRole::Popover, false) => (1.6755160819145565, 1.8500490071139892),
            _ => (1.3089969389957472, 1.3089969389957472),
        };
        let effects = Effects {
            face_opacity: 1.0, bleed_blend_mode: 1, shadow_offset: spec.shadow.offset,
            ring_vertical_offset: 0.0, ring_stroke_width: 0.0, ring_radius: 0.0,
            ring_opacity: 0.0, ring_mask: 0.0, holding_tone: 0, headroom_fraction: 0.0,
            sdr_shadow_distances: [-2.5,-1.5], aberration_amount: 0.1, aberration_height: 20.0,
            aberration_offset: 0.0, aberration_angle: 0.7, sdr_white_value: 1.0,
            clamp_limit: 1.0, clamp_override: 0.0, context_gamma: 2.2,
            preserve_hue: 0, force_preserve_hue: false,
            highlights: Highlights {
                height: if layer.specular { 1.0 } else { 0.0 },
                angle: std::f64::consts::FRAC_PI_2,
                spread: dark_spread,
                spread_sdr: dark_spread_sdr,
                amount: 0.4,
                effect_offset: layer.highlight_offset.unwrap_or(-1.0),
                color_bias: -0.25,
            },
        };
        let parameters = BackgroundParameters {
            blur: resolved.blur, refraction: resolved.refraction, edge_bleed: resolved.edge_bleed,
            shadow: resolved.shadow, fill: resolved.fill,
            colors: ColorMatrices { face: face_matrix, bleed: bleed_matrix, shadow: shadow_matrix },
            effects, geometry, render_scale: scale,
            shadow_face_opacity: 0.0, face_color_matrix_max_luma_complement: 1.0-face_max_luma,
        };
        let style = if dock { BackgroundStyle::default() } else { self.style };
        let material = BackgroundMaterial::with_style(&parameters,style)?;
        Ok(PreparedSdrMaterial { material, blur_levels })
    }
}

// Original RegularDiffusionAmount at 0x23f56ea08. The retained color blend
// operates in encoded RGB and the two halves have different alpha endpoints.
fn regular_diffusion(
    fill: &mut BlurFill, face: &mut YccColor, face_max_luma: &mut f32, amount: f32,
    dimensions: Dimensions, dark: bool,
) -> Result<()> {
    if (amount - 0.5).abs() < f32::from_bits(0x39b504f3) {
        return Ok(());
    }
    let negative = amount < 0.5;
    let x = f64::from(amount);
    let t = if negative { x+x } else { (x-0.5)+(x-0.5) };
    let t = t.clamp(0.0,1.0);
    let low = (1.0-t) as f32;
    let high = t as f32;
    let edge_mix = if negative { (0.9f32*0.75)*low + 0.9*high } else { 0.9*high + 0.9*low };
    if dark { fill.darken = edge_mix; } else { fill.lighten = edge_mix; }
    fill.normal = if negative { 0.5*high } else { 0.5*low + high };
    let [red,_,_,alpha] = face.fill;
    let other = if dark { face.black } else { 1.0 };
    let a = if negative { 0.0 } else { 0.5 };
    let (base_weight, other_weight) = if negative { (high,low) } else { (low,high) };
    let encoded = original_encode(red);
    let other = original_encode(original_decode(other));
    let c = original_decode(other*other_weight + encoded*base_weight);
    face.fill = [c,c,c,a*other_weight + alpha*base_weight];
    if dark && negative {
        *face_max_luma = regular_dark_luma(dimensions,0.45)?*low + *face_max_luma*high;
    }
    Ok(())
}

fn regular_dark_luma(dimensions: Dimensions, upper: f32) -> Result<f32> {
    let size = dimensions.largest_area_minimum.or(dimensions.minimum).unwrap_or(48.0);
    if !size.is_finite() { return Err("finite recipe dimension required".into()); }
    let t = ((size as f32 - 64.0) / 40.0).clamp(0.0,1.0);
    Ok(upper + (0.6f32-upper)*(1.0-t))
}

fn linear_srgb(value: f32) -> f32 {
    if value <= 0.04045 { value/12.92 } else { ((value+0.055)/1.055).powf(2.4) }
}

fn original_decode(value: f32) -> f32 {
    let magnitude = value.abs();
    let linear = if magnitude <= f32::from_bits(0x3d25aee6) {
        magnitude * f32::from_bits(0x3d9e8391)
    } else if magnitude == 1.0 { 1.0 } else {
        (magnitude * f32::from_bits(0x3f72a76f) + f32::from_bits(0x3d55891a)).powf(2.4)
    };
    if value>0.0 { linear } else { -linear }
}

fn original_encode(value: f32) -> f32 {
    let magnitude = value.abs();
    let encoded = if magnitude <= f32::from_bits(0x3b4d2e1c) {
        magnitude * 12.92
    } else if magnitude == 1.0 { 1.0 } else {
        magnitude.powf(f32::from_bits(0x3ed55555))*1.055 - 0.055
    };
    if value>0.0 { encoded } else { -encoded }
}
