//! Build recovered 26A434 LPF shader inputs from resolved material parameters.
mod pack;
mod color;
pub mod spec;
pub mod recipe;
pub mod adaptive;

use crate::{Result, scene::{Material, MaterialKind}};
pub use color::{YccColor, apply_dodge_fill, linear_fill_matrix};

pub struct Blur {
    pub radius: f32,
    pub distances: [f32; 4],
    pub opacities: [f32; 4],
    pub opacity: f32,
}

pub struct Refraction {
    pub inner_height: f64,
    pub inner_amount: f64,
    pub outer_height: f64,
    pub outer_amount: f64,
    pub outer_distances: [f64; 2],
    pub outer_opacity: f32,
}

pub struct EdgeBleed {
    pub amount: f64,
    pub height: f64,
    pub blur_radius: f64,
    pub opacity: f32,
    pub distances: [f32; 2],
}

pub struct Shadow {
    pub amount: f64,
    pub height: f64,
    pub blur_radius: f64,
    pub radius: f64,
    pub inset: f64,
    pub opacity: f32,
    pub vibrancy_contribution: f64,
}

pub struct BlurFill {
    pub radius: f32,
    pub lighten: f32,
    pub darken: f32,
    pub normal: f32,
}

/// The RGB rows select the R, G, B and translation columns of each matrix.
/// Values are in the resolved CA/sRGB working space.
pub struct ColorMatrices {
    pub face: [[f32; 5]; 4],
    pub bleed: [[f32; 5]; 4],
    pub shadow: [[f32; 5]; 4],
}

pub struct Geometry {
    /// CA linear transform (a, b, c, d).
    pub transform: [f32; 4],
    pub source_size: [u32; 2],
    pub target_size: [u32; 2],
    pub source_scale: f32,
    pub output_scale: f32,
    pub sdf_transform: Option<[f32; 4]>,
}

pub struct Highlights {
    pub height: f64,
    pub angle: f64,
    pub spread: f64,
    pub spread_sdr: f64,
    pub amount: f64,
    pub effect_offset: f64,
    pub color_bias: f64,
}

pub struct Effects {
    pub face_opacity: f64,
    pub bleed_blend_mode: i32,
    pub shadow_offset: [f64; 2],
    pub ring_vertical_offset: f64,
    pub ring_stroke_width: f64,
    pub ring_radius: f64,
    pub ring_opacity: f64,
    pub ring_mask: f64,
    pub holding_tone: i32,
    pub headroom_fraction: f32,
    pub sdr_shadow_distances: [f64; 2],
    pub aberration_amount: f64,
    pub aberration_height: f64,
    pub aberration_offset: f64,
    pub aberration_angle: f64,
    pub sdr_white_value: f64,
    pub clamp_limit: f64,
    pub clamp_override: f32,
    pub context_gamma: f32,
    pub preserve_hue: i32,
    pub force_preserve_hue: bool,
    pub highlights: Highlights,
}

/// Resolved material and rendering context for one background draw.
pub struct BackgroundParameters {
    pub blur: Blur,
    pub refraction: Refraction,
    pub edge_bleed: EdgeBleed,
    pub shadow: Shadow,
    pub fill: Option<BlurFill>,
    pub colors: ColorMatrices,
    pub effects: Effects,
    pub geometry: Geometry,
    pub render_scale: f32,
    pub shadow_face_opacity: f32,
    pub face_color_matrix_max_luma_complement: f32,
}

pub struct BackgroundMaterial {
    uniforms: [u8; 320],
    extension: [u8; 28],
}

/// Optional presentation controls applied to the recovered shader inputs.
pub struct BackgroundStyle {
    pub refraction_gain: f64,
    /// Explicit shader amount selected by the host control (for example focus).
    pub key_fill_amount: Option<f32>,
}

impl Default for BackgroundStyle {
    fn default() -> Self {
        Self { refraction_gain: 1.0, key_fill_amount: None }
    }
}

impl BackgroundMaterial {
    pub fn new(parameters: &BackgroundParameters) -> Result<Self> {
        Self::with_style(parameters, BackgroundStyle::default())
    }

    pub fn with_style(parameters: &BackgroundParameters, style: BackgroundStyle) -> Result<Self> {
        pack::background(parameters, style)
    }

    pub fn material(&self, kind: MaterialKind) -> Result<Material<'_>> {
        if matches!(kind, MaterialKind::Foreground) {
            return Err("background parameters require a background material kind".into());
        }
        Ok(Material { kind, uniforms: &self.uniforms, extension: &self.extension })
    }
}
