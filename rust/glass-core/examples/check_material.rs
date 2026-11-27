//! Pack typed material fixtures from stdin and return 348 LPF bytes per input.
use liquid_glass_core::{material::{*, spec::{Dimensions, MaterialSpec, ResolvedComponents}}, scene::MaterialKind};
use std::io::{self, BufRead, BufWriter, Write};
use liquid_glass_core::{
    material::recipe::{PreparedSdrMaterial, RecipeRole, SdrRecipe},
    overlay::Appearance,
    raster::{Scale, Viewport},
};

struct Fixture<'a>(std::str::SplitWhitespace<'a>);

impl Fixture<'_> {
    fn next<T: std::str::FromStr>(&mut self) -> Result<T, &'static str> {
        self.0.next().ok_or("missing material fixture field")?.parse().map_err(|_| "invalid material fixture field")
    }

    fn array<T: std::str::FromStr + Default + Copy, const N: usize>(&mut self) -> Result<[T; N], &'static str> {
        let mut values = [T::default(); N];
        for item in &mut values {
            *item = self.next()?;
        }
        Ok(values)
    }

    fn matrix(&mut self) -> Result<[[f32; 5]; 4], &'static str> {
        match self.next::<u8>()? {
            0 => Ok([self.array()?, self.array()?, self.array()?, self.array()?]),
            1 => YccColor {
                white: self.next()?, black: self.next()?, saturation: self.next()?, fill: self.array()?,
            }.matrix().map_err(|_| "invalid YCC fixture"),
            2 => Ok(linear_fill_matrix(self.array()?)),
            _ => Err("unknown color fixture encoding"),
        }
    }

    fn recipe(&mut self, adaptive: bool) -> Result<PreparedSdrMaterial, Box<dyn std::error::Error>> {
        let role = match self.next::<u8>()? {
            0 => RecipeRole::Dock,
            1 => RecipeRole::Regular,
            2 => RecipeRole::Clear,
            _ => return Err("unknown recipe role".into()),
        };
        let appearance = if self.next::<bool>()? { Appearance::Dark } else { Appearance::Light };
        let dimensions = Dimensions { minimum: Some(self.next()?), largest_area_minimum: Some(self.next()?) };
        let diffusion = self.next()?;
        let scale = match self.next::<u8>()? {
            1 => Scale::One,
            2 => Scale::Two,
            _ => return Err("invalid recipe fixture scale".into()),
        };
        let viewport = Viewport::new(self.next()?, self.next()?, scale)?;
        let tint = self.next::<bool>()?.then(|| self.array()).transpose()?;
        let style = BackgroundStyle {
            refraction_gain: self.next()?,
            key_fill_amount: self.next::<bool>()?.then(|| self.next()).transpose()?,
        };
        let luminance = if adaptive { Some(self.next()?) } else { None };
        if self.0.next().is_some() { return Err("unexpected recipe fixture suffix".into()); }
        Ok(SdrRecipe { role, appearance, dimensions, diffusion, tint, style }.build_with_luminance(viewport, luminance)?)
    }

    fn components(&mut self) -> Result<ResolvedComponents, Box<dyn std::error::Error>> {
        if self.next::<bool>()? {
            let hex = self.0.next().ok_or("missing Spec bytes")?;
            if hex.len()!=0xe00 || !hex.is_ascii() { return Err("invalid Spec bytes".into()); }
            let raw = (0..hex.len()).step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i+2],16))
                .collect::<Result<Vec<_>,_>>()?;
            let dims = Dimensions {minimum:Some(self.next()?),largest_area_minimum:Some(self.next()?)};
            let spec = MaterialSpec::from_26a434_snapshot(&raw)?;
            let mut components = spec.resolve(dims,self.next()?,self.next()?,self.next()?)?;
            components.canonicalize_inactive_radii();
            return Ok(components);
        }
        let blur = Blur { radius: self.next()?, distances: self.array()?, opacities: self.array()?, opacity: self.next()? };
        let refraction = Refraction {
            inner_height: self.next()?, inner_amount: self.next()?, outer_height: self.next()?, outer_amount: self.next()?,
            outer_distances: self.array()?, outer_opacity: self.next()?,
        };
        let edge_bleed = EdgeBleed { amount: self.next()?, height: self.next()?, blur_radius: self.next()?, opacity: self.next()?, distances: self.array()? };
        let shadow = Shadow { amount: self.next()?, height: self.next()?, blur_radius: self.next()?, radius: self.next()?,
            inset: self.next()?, opacity: self.next()?, vibrancy_contribution: self.next()? };
        let fill = self.next::<bool>()?.then(|| -> Result<BlurFill, &'static str> {
            Ok(BlurFill { radius: self.next()?, lighten: self.next()?, darken: self.next()?, normal: self.next()? })
        }).transpose()?;
        Ok(ResolvedComponents { blur, refraction, edge_bleed, shadow, fill })
    }

    fn parameters(&mut self) -> Result<(BackgroundParameters, BackgroundStyle), Box<dyn std::error::Error>> {
        let render_scale = self.next()?;
        let ResolvedComponents { blur, refraction, edge_bleed, shadow, fill } = self.components()?;
        let colors = ColorMatrices { face: self.matrix()?, bleed: self.matrix()?, shadow: self.matrix()? };
        let effects = Effects {
            face_opacity: self.next()?, bleed_blend_mode: self.next()?, shadow_offset: self.array()?,
            ring_vertical_offset: self.next()?, ring_stroke_width: self.next()?, ring_radius: self.next()?,
            ring_opacity: self.next()?, ring_mask: self.next()?, holding_tone: self.next()?,
            headroom_fraction: self.next()?, sdr_shadow_distances: self.array()?,
            aberration_amount: self.next()?, aberration_height: self.next()?, aberration_offset: self.next()?,
            aberration_angle: self.next()?, sdr_white_value: self.next()?, clamp_limit: self.next()?,
            clamp_override: self.next()?, context_gamma: self.next()?, preserve_hue: self.next()?, force_preserve_hue: self.next()?,
            highlights: Highlights { height: self.next()?, angle: self.next()?, spread: self.next()?, spread_sdr: self.next()?,
                amount: self.next()?, effect_offset: self.next()?, color_bias: self.next()? },
        };
        let geometry = Geometry {
            transform: self.array()?, source_size: self.array()?, target_size: self.array()?,
            source_scale: self.next()?, output_scale: self.next()?,
            sdf_transform: self.next::<bool>()?.then(|| self.array()).transpose()?,
        };
        let parameters = BackgroundParameters {
            blur, refraction, edge_bleed, shadow, fill, colors, effects, geometry, render_scale,
            shadow_face_opacity: self.next()?, face_color_matrix_max_luma_complement: self.next()?,
        };
        let style = BackgroundStyle {
            refraction_gain: self.next()?,
            key_fill_amount: self.next::<bool>()?.then(|| self.next()).transpose()?,
        };
        if self.0.next().is_some() {
            return Err("unexpected material fixture suffix".into());
        }
        Ok((parameters, style))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(io::stdout().lock());
    let mode = std::env::args().nth(1);
    for line in io::stdin().lock().lines() {
        let line = line?;
        let mut fixture = Fixture(line.split_whitespace());
        if mode.as_deref() == Some("--color") {
            for value in fixture.matrix()?.into_iter().flatten() {
                output.write_all(&value.to_le_bytes())?;
            }
            continue;
        }
        if matches!(mode.as_deref(), Some("--recipe" | "--recipe-adaptive")) {
            let prepared = fixture.recipe(mode.as_deref() == Some("--recipe-adaptive"))?;
            let material = prepared.material.material(MaterialKind::All)?;
            output.write_all(material.uniforms)?;
            output.write_all(material.extension)?;
            output.write_all(&prepared.blur_levels.to_le_bytes())?;
            continue;
        }
        let (parameters, style) = fixture.parameters()?;
        let packed = BackgroundMaterial::with_style(&parameters, style)?;
        let material = packed.material(MaterialKind::All)?;
        output.write_all(material.uniforms)?;
        output.write_all(material.extension)?;
        output.flush()?;
    }
    Ok(())
}
