//! 26A434 numeric Spec resolvers. Recipe selection and adaptivity precede or follow these blocks.
mod snapshot;

use crate::Result;
use super::{Blur, BlurFill, EdgeBleed, Refraction, Shadow};

#[derive(Clone, Copy, Default)]
pub struct Dimensions {
    pub minimum: Option<f64>,
    pub largest_area_minimum: Option<f64>,
}

impl Dimensions {
    fn values(self) -> Result<[f64; 2]> {
        let minimum = self.minimum.unwrap_or(48.0);
        let largest = self.largest_area_minimum.unwrap_or(minimum);
        Ok([finite(minimum)?, finite(largest)?])
    }
}

#[derive(Clone, Copy)]
pub enum ParameterScale<T> {
    Constant(T),
    Linear { slope: T, intercept: T, lower: Option<T>, upper: Option<T> },
    ClampedLinear { input: [f64; 2], output: [T; 2], upper_tail: Option<UpperTail<T>> },
}

#[derive(Clone, Copy)]
pub struct UpperTail<T> {
    pub output: T,
    pub transition: f64,
}

trait ScaleValue: Copy + PartialOrd + std::ops::Add<Output=Self>
    + std::ops::Sub<Output=Self> + std::ops::Mul<Output=Self>
{
    fn cast(value: f64) -> Self;
    fn checked(self) -> Result<Self>;
}

impl ScaleValue for f32 {
    fn cast(value: f64) -> Self { value as f32 }
    fn checked(self) -> Result<Self> { finite_f32(self) }
}

impl ScaleValue for f64 {
    fn cast(value: f64) -> Self { value }
    fn checked(self) -> Result<Self> { finite(self) }
}

fn finite(value: f64) -> Result<f64> {
    if value.is_finite() { Ok(value) } else { Err("finite spec value required".into()) }
}

fn finite_f32(value: f32) -> Result<f32> {
    if value.is_finite() { Ok(value) } else { Err("finite Float32 spec value required".into()) }
}

fn signed_length(value: f64) -> Result<f64> {
    // The source squares and square-roots the distance before applying its sign.
    let length = finite(value * value)?.sqrt();
    Ok(if value < 0.0 { -length } else { length })
}

fn fraction(value: f64, start: f64, end: f64) -> Result<f64> {
    const MINIMUM_SPAN: f64 = 1.0 / 67_108_864.0;
    let span = signed_length(end - start)?;
    if span <= MINIMUM_SPAN {
        Ok(0.0)
    } else {
        Ok(signed_length(value - start)? / span)
    }
}

fn resolve_scale<T: ScaleValue>(scale: ParameterScale<T>, dimension: f64) -> Result<T> {
    finite(dimension)?;
    match scale {
        ParameterScale::Constant(value) => value.checked(),
        ParameterScale::Linear { slope, intercept, lower, upper } => {
            let product = (T::cast(signed_length(dimension)?).checked()? * slope.checked()?).checked()?;
            let mut value = (product + intercept.checked()?).checked()?;
            if let Some(lower) = lower && value < lower.checked()? {
                value = lower;
            }
            if let Some(upper) = upper && value > upper.checked()? {
                value = upper;
            }
            Ok(value)
        }
        ParameterScale::ClampedLinear { input: [start, end], output: [first, last], upper_tail } => {
            if finite(end)? < finite(start)? {
                return Err("unordered material scale input range".into());
            }
            first.checked()?;
            last.checked()?;
            if let Some(tail) = upper_tail {
                tail.output.checked()?;
                if finite(tail.transition)? < 0.0 {
                    return Err("negative material scale transition".into());
                }
            }
            let t = fraction(dimension, start, end)?;
            let increasing = first <= last;
            let (base, limit, t) = if let Some(tail) = upper_tail && t > 1.0 {
                if increasing && tail.output < last || !increasing && tail.output > first {
                    return Err("invalid original material scale upper tail".into());
                }
                let progress = fraction(dimension, end, finite(end + tail.transition)?)?;
                let t = if increasing { progress } else { 1.0 - progress }.clamp(0.0, 1.0);
                let eased = (t * t) * (3.0 - (t + t));
                if increasing { (last, tail.output, eased) } else { (tail.output, first, eased) }
            } else {
                let t = t.clamp(0.0, 1.0);
                if increasing { (first, last, t) } else { (last, first, 1.0 - t) }
            };
            let delta = (limit - base).checked()?;
            (base + (delta * T::cast(t)).checked()?).checked()
        }
    }
}

#[derive(Clone, Copy)]
pub enum BlurDistance {
    Fixed(f32),
    Inset(f32),
    Outset(f32),
}

pub struct BlurSpec {
    pub opacity: f32,
    pub opacities: [f32; 4],
    pub distances: [BlurDistance; 4],
    pub radius: ParameterScale<f32>,
    pub outer_refraction_blur_radius: Option<f32>,
}

impl BlurSpec {
    pub fn resolve(&self, dimensions: Dimensions, layers: u64, applicable: bool) -> Result<Blur> {
        let [minimum, largest] = dimensions.values()?;
        let mut radius = finite_f32(resolve_scale(self.radius, largest)? * 2.0)?;
        let mut opacities = self.opacities;
        finite_f32(self.opacity)?;
        for value in opacities { finite_f32(value)?; }
        if let Some(outer) = self.outer_refraction_blur_radius && layers & 0x10 != 0 {
            let outer = finite_f32(outer * 2.0)?;
            let divisor = if radius <= outer { outer } else { radius };
            if divisor == 0.0 {
                return Err("zero original blur opacity divisor".into());
            }
            for (index, value) in opacities.iter_mut().enumerate() {
                let numerator = if index < 2 { radius } else { outer };
                *value = finite_f32(finite_f32(numerator * *value)? / divisor)?;
            }
            radius = divisor;
        }

        let mut distances = [0.0; 4];
        for (index, distance) in self.distances.iter().enumerate() {
            let mut value = match *distance {
                BlurDistance::Fixed(value) if !value.is_nan() => {
                    distances[index] = value;
                    continue;
                }
                BlurDistance::Fixed(_) => return Err("invalid fixed blur distance".into()),
                BlurDistance::Inset(value) => -finite_f32(finite_f32(value)? * minimum as f32)?,
                BlurDistance::Outset(value) => finite_f32(finite_f32(value)? * largest as f32)?,
            };
            if applicable {
                let next = self.distances[index+1..].iter().find_map(|distance| match *distance {
                    BlurDistance::Fixed(fixed) if fixed < value => Some(fixed),
                    _ => None,
                });
                let bound = next.or_else(|| self.distances[..index].iter().find_map(|distance| match *distance {
                    BlurDistance::Fixed(fixed) if value < fixed => Some(fixed),
                    _ => None,
                }));
                if let Some(bound) = bound { value = bound; }
            }
            distances[index] = value;
        }
        Ok(Blur { radius, distances, opacities, opacity: self.opacity })
    }

    /// ClearDiffusionAmount, also used in the bounded Dock recipe.
    pub fn apply_clear_diffusion(
        &mut self, amount: Option<f32>, applicable: bool, fill: &mut BlurFill,
    ) -> Result<()> {
        let Some(amount) = amount.filter(|_| applicable) else { return Ok(()); };
        let amount = finite_f32(amount)?;
        if amount < 0.5 {
            let factor = amount + amount;
            match &mut self.radius {
                ParameterScale::Constant(radius) => *radius = finite_f32(factor * *radius)?,
                ParameterScale::ClampedLinear { output, .. } => {
                    *output = [finite_f32(output[0] * factor)?, finite_f32(output[1] * factor)?];
                }
                ParameterScale::Linear { .. } => {}
            }
        } else if amount > 0.5 {
            let normal = finite_f32((amount - 0.5) + (amount - 0.5))?;
            *fill = BlurFill { radius: 16.0, lighten: 0.0, darken: 0.0, normal };
        }
        Ok(())
    }
}

pub struct BoundedDistance {
    pub normalized: f64,
    pub bounds: [f64; 2],
}

impl BoundedDistance {
    fn resolve(&self, dimension: f64) -> Result<f64> {
        let [lower, upper] = self.bounds;
        if lower.is_nan() || upper.is_nan() {
            return Err("invalid refraction distance bounds".into());
        }
        let computed = finite(finite(self.normalized)? * dimension)?;
        let value = if computed >= lower { computed } else { lower };
        Ok(if upper < value { upper } else { value })
    }
}

pub struct RefractionSpec {
    pub inner_height: BoundedDistance,
    pub inner_amount: BoundedDistance,
    pub outer_height: BoundedDistance,
    pub outer_amount: BoundedDistance,
    pub outer_opacity: f32,
}

impl RefractionSpec {
    pub fn resolve(&self, dimensions: Dimensions, pixel_length: f64, applicable: bool) -> Result<Refraction> {
        let [minimum, largest] = dimensions.values()?;
        finite(pixel_length)?;
        Ok(Refraction {
            inner_height: self.inner_height.resolve(minimum)?,
            inner_amount: self.inner_amount.resolve(minimum)?,
            outer_height: self.outer_height.resolve(largest)?,
            outer_amount: self.outer_amount.resolve(largest)?,
            outer_distances: [-1.0, if applicable { 0.0 } else { finite(pixel_length - 1.0)? }],
            outer_opacity: finite_f32(self.outer_opacity)?,
        })
    }
}

pub struct EdgeBleedSpec {
    pub opacity: ParameterScale<f32>,
    pub normalized_height: f64,
    pub normalized_amount: f64,
    pub normalized_blur_radius: f64,
    pub max_blur_radius: f64,
    pub ycc: [u8; 69],
    pub use_darken_blending: bool,
}

impl EdgeBleedSpec {
    pub fn resolve(&self, dimensions: Dimensions) -> Result<EdgeBleed> {
        let [_, largest] = dimensions.values()?;
        let radius = finite(self.normalized_blur_radius * largest)?;
        if self.max_blur_radius.is_nan() {
            return Err("invalid edge blur radius cap".into());
        }
        // The native comparison selects the cap on equality, including signed zero.
        let blur_radius = if radius < self.max_blur_radius { radius } else { self.max_blur_radius };
        Ok(EdgeBleed {
            amount: finite(self.normalized_amount * largest)?,
            height: finite(self.normalized_height * largest)?,
            blur_radius,
            opacity: resolve_scale(self.opacity, largest)?,
            distances: [1.0, 0.0],
        })
    }
}

pub struct ShadowSpec {
    pub normalized_height: f64,
    pub normalized_amount: f64,
    pub blur_radius: f64,
    pub opacity: ParameterScale<f32>,
    pub radius: ParameterScale<f64>,
    pub offset: [f64; 2],
    pub ycc: [u8; 69],
    pub vibrancy_contribution: ParameterScale<f64>,
}

impl ShadowSpec {
    pub fn resolve(&self, dimensions: Dimensions) -> Result<Shadow> {
        let [_, largest] = dimensions.values()?;
        for offset in self.offset { finite(offset)?; }
        let amount = finite(self.normalized_amount * largest)?.min(75.0);
        Ok(Shadow {
            amount,
            height: finite(self.normalized_height * largest)?,
            blur_radius: finite(self.blur_radius)?,
            radius: resolve_scale(self.radius, largest)?,
            inset: 0.0,
            opacity: resolve_scale(self.opacity, largest)?,
            vibrancy_contribution: resolve_scale(self.vibrancy_contribution, largest)?,
        })
    }
}

/// Numeric slices used by the SDR background path, decoded from an original Spec.
pub struct MaterialSpec {
    pub blur: BlurSpec,
    pub refraction: RefractionSpec,
    pub edge_bleed: EdgeBleedSpec,
    pub shadow: ShadowSpec,
    pub fill: BlurFill,
}

pub struct ResolvedComponents {
    pub blur: Blur,
    pub refraction: Refraction,
    pub edge_bleed: EdgeBleed,
    pub shadow: Shadow,
    pub fill: Option<BlurFill>,
}

impl ResolvedComponents {
    /// Remove the unused radii and blur opacity of inactive background
    /// components. These are the numeric rules reached by the full-layer SDR
    /// demo, independently of the renderer's shader-side opacity checks.
    pub fn canonicalize_inactive_radii(&mut self) {
        if self.blur.radius == 0.0 {
            self.blur.opacity = 0.0;
        } else if self.blur.opacity == 0.0 {
            self.blur.radius = 0.0;
        }
        if self.edge_bleed.opacity == 0.0 {
            self.edge_bleed.blur_radius = 0.0;
        }
        if self.shadow.opacity == 0.0 {
            self.shadow.blur_radius = 0.0;
            self.shadow.radius = 0.0;
            self.shadow.vibrancy_contribution = 0.0;
        }
    }
}

impl MaterialSpec {
    pub fn from_26a434_snapshot(bytes: &[u8]) -> Result<Self> {
        snapshot::decode(bytes)
    }

    pub fn resolve(
        &self, dimensions: Dimensions, layers: u64, pixel_length: f64, applicable: bool,
    ) -> Result<ResolvedComponents> {
        let BlurFill { radius, lighten, darken, normal } = self.fill;
        let fill = [radius, lighten, darken, normal];
        for value in fill { finite_f32(value)?; }
        Ok(ResolvedComponents {
            blur: self.blur.resolve(dimensions, layers, applicable)?,
            refraction: self.refraction.resolve(dimensions, pixel_length, applicable)?,
            edge_bleed: self.edge_bleed.resolve(dimensions)?,
            shadow: self.shadow.resolve(dimensions)?,
            fill: fill.into_iter().any(|v| v != 0.0)
                .then_some(BlurFill { radius, lighten, darken, normal }),
        })
    }
}
