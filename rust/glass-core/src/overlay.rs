//! SDR host composition using the extracted button palette and glow data.
use crate::{Result, Size, interaction::Glow};
mod data {
    include!("overlay_data.rs");
}
use data::{FILLS, MASK_CENTER, MASK_SIZE, MATRICES, RASTER_SCALE};

static MASK: &[u8; MASK_SIZE[0] * MASK_SIZE[1]] = include_bytes!("glow-mask.bin");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Idle,
    Rollover,
    Pressed,
}

pub use crate::raster::Scale;

fn validate(size: Size, image: &[[f32; 4]], coverage: &[f32]) -> Result<()> {
    size.validate()?;
    if image.len() != size.pixels() || coverage.len() != size.pixels() {
        return Err("overlay requires matching raster and coverage dimensions".into());
    }
    if coverage.iter().any(|v| !v.is_finite())
        || image.iter().any(|p| !p.iter().all(|v| v.is_finite()) || !(0.0..=1.0).contains(&p[3]))
    {
        return Err("finite raster with alpha in [0,1] and finite coverage required".into());
    }
    Ok(())
}

pub(crate) fn decode(pixel: [f32; 4]) -> [f32; 4] {
    let alpha = pixel[3];
    let mut linear = pixel;
    for channel in &mut linear[..3] {
        let value = if alpha == 0.0 { 0.0 } else { *channel / alpha };
        *channel = alpha * if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        };
    }
    linear
}

pub(crate) fn encode(mut linear: [f32; 4]) -> [f32; 4] {
    let alpha = linear[3];
    for channel in &mut linear[..3] {
        let value = if alpha == 0.0 { 0.0 } else { *channel / alpha };
        *channel = alpha * if value <= 0.0031308 {
            12.92 * value
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
    }
    linear
}

fn over(front: [f32; 4], back: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|c| front[c] + back[c] * (1.0 - front[3]))
}

/// Apply the original semantic fill for mouse rollover or press.
/// Input and output use premultiplied sRGB. Coverage is sampled per raster pixel.
pub fn composite_fill(
    size: Size,
    image: &mut [[f32; 4]],
    coverage: &[f32],
    appearance: Appearance,
    fill: Fill,
) -> Result<()> {
    validate(size, image, coverage)?;
    let index = match fill {
        Fill::Idle => return Ok(()),
        Fill::Rollover => 0,
        Fill::Pressed => 1,
    };
    let color = FILLS[appearance as usize][index];
    for (pixel, &mask) in image.iter_mut().zip(coverage) {
        let alpha = mask.clamp(0.0, 1.0) * color[3] as f32;
        let front = std::array::from_fn(|c| {
            if c == 3 { alpha } else { (color[c] * f64::from(alpha)) as f32 }
        });
        *pixel = encode(over(front, decode(*pixel)));
    }
    Ok(())
}

fn filter(pixel: [f32; 4], opacity: f32, matrix: &[[f32; 5]; 4]) -> [f32; 4] {
    if opacity == 0.0 {
        return pixel;
    }
    let back = decode(pixel);
    let divisor = back[3].max(1.0e-6);
    let rgb: [f32; 3] = std::array::from_fn(|c| back[c] / divisor);
    let adjusted = matrix.map(|row| {
        rgb[0] * row[0] + rgb[1] * row[1] + rgb[2] * row[2] + back[3] * row[3] + row[4]
    });
    let alpha = adjusted[3].clamp(0.0, 1.0);
    let front = std::array::from_fn(|c| {
        if c == 3 { alpha * opacity } else { adjusted[c] * alpha * opacity }
    });
    encode(over(front, back))
}

fn sample_mask(x: f32, y: f32) -> f32 {
    if !(-1.0..MASK_SIZE[1] as f32).contains(&x) || !(-1.0..MASK_SIZE[0] as f32).contains(&y) {
        return 0.0;
    }
    let (i, j) = (x.floor() as isize, y.floor() as isize);
    let (fx, fy) = (f64::from(x) - i as f64, f64::from(y) - j as f64);
    let mut value = 0.0f32;
    for (dx, dy, weight) in [
        (0, 0, (1.0 - fx) * (1.0 - fy)),
        (1, 0, fx * (1.0 - fy)),
        (0, 1, (1.0 - fx) * fy),
        (1, 1, fx * fy),
    ] {
        let (u, v) = (i + dx, j + dy);
        if u >= 0 && v >= 0 && (u as usize) < MASK_SIZE[1] && (v as usize) < MASK_SIZE[0] {
            let sample = f32::from(MASK[v as usize * MASK_SIZE[1] + u as usize]) / 255.0;
            value = (f64::from(value) + f64::from(sample) * weight) as f32;
        }
    }
    value
}

/// Composite the large press glow followed by the local light spot.
/// Origin, spot position and diameter use logical points. Coverage uses image pixels.
pub fn composite_glow(
    size: Size,
    image: &mut [[f32; 4]],
    coverage: &[f32],
    appearance: Appearance,
    glow: &Glow,
    origin: [f64; 2],
    scale: Scale,
) -> Result<()> {
    validate(size, image, coverage)?;
    if ![glow.opacity, glow.big_opacity].iter().all(|a| (0.0..=1.0).contains(a))
        || ![glow.scale, glow.diameter].iter().all(|v| v.is_finite() && *v >= 0.0)
        || !origin.iter().chain(&glow.position).all(|v| v.is_finite())
    {
        return Err("invalid glow presentation".into());
    }
    let pixel_scale = scale.factor() as f32;
    let local = glow.opacity > 0.0 && glow.scale > 0.0 && glow.diameter > 0.0;
    let center: [f32; 2] = std::array::from_fn(|i| (origin[i] + glow.position[i]) as f32);
    let ratio = if local { (RASTER_SCALE * 114.0 / glow.diameter) as f32 } else { 0.0 };
    if local && (!center.iter().all(|v| v.is_finite()) || !ratio.is_finite()
        || (glow.scale as f32) == 0.0 || !(glow.scale as f32).is_finite())
    {
        return Err("glow coordinates exceed raster precision".into());
    }
    let width = size.width as usize;
    for (i, (pixel, &mask)) in image.iter_mut().zip(coverage).enumerate() {
        let coverage = mask.clamp(0.0, 1.0);
        if coverage == 0.0 {
            continue;
        }
        *pixel = filter(*pixel, coverage * glow.big_opacity as f32, &MATRICES[0]);
        if local {
            let x = ((i % width) as f32 + 0.5) / pixel_scale;
            let y = ((i / width) as f32 + 0.5) / pixel_scale;
            let u = ((x - center[0]) / glow.scale as f32) * ratio + MASK_CENTER[0] - 0.5;
            let v = ((y - center[1]) / glow.scale as f32) * ratio + MASK_CENTER[1] - 0.5;
            let alpha = sample_mask(u, v) * glow.opacity as f32 * coverage;
            *pixel = filter(*pixel, alpha, &MATRICES[1 + appearance as usize]);
        }
    }
    Ok(())
}
