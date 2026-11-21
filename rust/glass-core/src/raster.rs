//! Host contract for logical scene size, SDR raster pixels and content updates.
use crate::{Raster, Result, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    One,
    Two,
}

impl Scale {
    pub const fn factor(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// Integral point bounds for the viewport. Individual geometry can be fractional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    points: Size,
    pixels: Size,
    scale: Scale,
}

impl Viewport {
    pub fn new(width_points: u32, height_points: u32, scale: Scale) -> Result<Self> {
        let points = Size::new(width_points, height_points)?;
        let factor = scale.factor();
        let pixels = Size::new(points.width * factor, points.height * factor)?;
        Ok(Self { points, pixels, scale })
    }

    pub fn points(self) -> Size { self.points }
    pub fn pixels(self) -> Size { self.pixels }
    pub fn scale(self) -> Scale { self.scale }

    /// Coordinate of a raster pixel's center in logical points, measured from the scene origin.
    pub fn pixel_center(self, x: u32, y: u32) -> Option<[f64; 2]> {
        (x < self.pixels.width && y < self.pixels.height).then(|| {
            let divisor = f64::from(self.scale.factor());
            [(f64::from(x) + 0.5) / divisor, (f64::from(y) + 0.5) / divisor]
        })
    }
}

/// Half-open pixel rectangle with the same origin as the supplied raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub(crate) fn full(bounds: Size) -> Self {
        Self { x: 0, y: 0, width: bounds.width, height: bounds.height }
    }

    pub(crate) fn check(self, bounds: Size) -> Result<()> {
        if self.width == 0 || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|x| x > bounds.width)
            || self.y.checked_add(self.height).is_none_or(|y| y > bounds.height)
        {
            return Err("dirty rectangle outside raster".into());
        }
        Ok(())
    }

    pub(crate) fn cover(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self { x, y, width: right - x, height: bottom - y }
    }

    pub(crate) fn align_quads(self, bounds: Size) -> Self {
        let x = self.x & !1;
        let y = self.y & !1;
        let right = (self.x + self.width).next_multiple_of(2).min(bounds.width);
        let bottom = (self.y + self.height).next_multiple_of(2).min(bounds.height);
        Self { x, y, width: right - x, height: bottom - y }
    }

    pub(crate) fn row_spans(self, bounds: Size) -> impl Iterator<Item=std::ops::Range<usize>> {
        (self.y..self.y + self.height).map(move |y| {
            let start = ((y * bounds.width + self.x) * 4) as usize;
            start..start + self.width as usize * 4
        })
    }

    /// Expand a dirty rectangle by the caller's conservative effect radius.
    pub fn expanded(self, radius_pixels: u32, bounds: Size) -> Result<Self> {
        bounds.validate()?;
        self.check(bounds)?;
        let x = self.x.saturating_sub(radius_pixels);
        let y = self.y.saturating_sub(radius_pixels);
        let right = (self.x + self.width).saturating_add(radius_pixels).min(bounds.width);
        let bottom = (self.y + self.height).saturating_add(radius_pixels).min(bounds.height);
        Ok(Self { x, y, width: right - x, height: bottom - y })
    }
}

/// Fully specified pixels which replace a dirty rectangle in RGBA row order.
pub struct Patch<'a> {
    pub rect: PixelRect,
    pub rgba: &'a [f32],
}

/// Pixels in linear sRGB with premultiplied alpha for the render passes.
/// Versions are strictly increasing per raster. The consumer must include the
/// raster identity as well as its version when reusing scene results.
#[derive(Clone)]
pub struct Content {
    viewport: Viewport,
    version: u64,
    rgba: Vec<f32>,
}

impl Content {
    pub fn new(viewport: Viewport, version: u64, rgba: Vec<f32>) -> Result<Self> {
        validate_pixels(&rgba, viewport.pixels.pixels())?;
        Ok(Self { viewport, version, rgba })
    }

    pub fn version(&self) -> u64 { self.version }
    pub fn viewport(&self) -> Viewport { self.viewport }
    pub fn raster(&self) -> Raster<'_> { Raster { size: self.viewport.pixels, rgba: &self.rgba } }

    /// Validate the whole update before writing any pixels. Overlapping patches
    /// are applied in caller order, with the last patch winning.
    pub fn update(&mut self, version: u64, patches: &[Patch<'_>]) -> Result<()> {
        if version <= self.version || patches.is_empty() {
            return Err("a newer version with at least one dirty rectangle is required".into());
        }
        let bounds = self.viewport.pixels;
        for patch in patches {
            patch.rect.check(bounds)?;
            validate_pixels(patch.rgba, patch.rect.width as usize * patch.rect.height as usize)?;
        }
        let stride = bounds.width as usize * 4;
        for patch in patches {
            let rect = patch.rect;
            let row_width = rect.width as usize * 4;
            let origin = rect.y as usize * stride + rect.x as usize * 4;
            for (row, src) in patch.rgba.chunks_exact(row_width).enumerate() {
                let start = origin + row * stride;
                self.rgba[start..start + row_width].copy_from_slice(src);
            }
        }
        self.version = version;
        Ok(())
    }
}

fn validate_pixels(rgba: &[f32], pixels: usize) -> Result<()> {
    if rgba.len() != pixels * 4 || rgba.chunks_exact(4).any(|p| {
        !p.iter().all(|v| v.is_finite()) || !(0.0..=1.0).contains(&p[3])
    }) {
        return Err("finite matching linear RGBA with alpha in [0,1] required".into());
    }
    Ok(())
}
