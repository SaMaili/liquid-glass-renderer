//! Ordered scene and interaction composition in the SDR sRGB frame contract.
mod cache;
pub use cache::{CacheStats, FrameCache, FrameStage};
use std::sync::Arc;
use crate::{
    Renderer, Result,
    interaction::Glow,
    overlay::{self, Appearance, Fill},
    raster::{Content, Viewport},
    scene::{self, Backdrop, Group, Shape},
};

/// Light-spot presentation and its origin in logical viewport points.
#[derive(Clone, Copy)]
pub struct LightSpot<'a> {
    pub glow: &'a Glow,
    pub origin: [f64; 2],
}

/// A single frame with premultiplied, sRGB-encoded pixels.
/// The scene renderer decodes each scene stage to linear light and re-encodes
/// the result, matching the color boundary between stages in the reference.
#[derive(Clone)]
pub struct Frame {
    viewport: Viewport,
    pixels: Arc<Vec<[f32; 4]>>,
}

impl Frame {
    pub fn new(viewport: Viewport, pixels: Vec<[f32; 4]>) -> Result<Self> {
        if pixels.len() != viewport.pixels().pixels() || pixels.iter().any(|p| {
            !p.iter().all(|v| v.is_finite()) || !(0.0..=1.0).contains(&p[3])
        }) {
            return Err("finite premultiplied sRGB frame with matching dimensions required".into());
        }
        Ok(Self { viewport, pixels: Arc::new(pixels) })
    }

    pub fn viewport(&self) -> Viewport { self.viewport }

    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.pixels
    }

    pub fn into_pixels(self) -> Vec<[f32; 4]> {
        Arc::unwrap_or_clone(self.pixels)
    }

    pub fn render_groups(
        &mut self,
        renderer: &mut Renderer,
        backdrop: Backdrop,
        groups: &[Group<'_>],
    ) -> Result<()> {
        self.check_target(renderer)?;
        let linear = self.pixels.iter().flat_map(|&p| overlay::decode(p)).collect();
        let content = Content::new(self.viewport, 0, linear)?;
        let output = scene::render(renderer, &content, backdrop, groups)?;
        let pixels = output.as_chunks::<4>().0.iter().map(|&p| overlay::encode(p)).collect();
        *self = Self::new(self.viewport, pixels)?;
        Ok(())
    }

    /// Derive coverage from a shape whose distance is expressed in logical
    /// points, then apply the semantic fill followed by the selected glow.
    pub fn interact(
        &mut self,
        renderer: &mut Renderer,
        shape: Shape<'_>,
        appearance: Appearance,
        fill: Fill,
        spot: Option<LightSpot<'_>>,
    ) -> Result<()> {
        self.check_target(renderer)?;
        if matches!(fill, Fill::Idle) && spot.is_none() {
            return Ok(());
        }
        let field = scene::field(renderer, shape)?;
        let scale = self.viewport.scale();
        let coverage: Vec<_> = field.chunks_exact(4)
            .map(|p| (0.5 - p[0] * scale.factor() as f32).clamp(0.0, 1.0))
            .collect();
        let size = self.viewport.pixels();
        let mut pixels = self.pixels.as_ref().clone();
        overlay::composite_fill(size, &mut pixels, &coverage, appearance, fill)?;
        if let Some(spot) = spot {
            overlay::composite_glow(size, &mut pixels, &coverage, appearance, spot.glow, spot.origin, scale)?;
        }
        *self = Self::new(self.viewport, pixels)?;
        Ok(())
    }

    fn check_target(&self, renderer: &Renderer) -> Result<()> {
        if renderer.size() != self.viewport.pixels() {
            return Err("frame viewport does not match render target".into());
        }
        Ok(())
    }
}
