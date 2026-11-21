//! Exact frame plan comparison and bounded reuse of unchanged scene prefixes.
use std::{rc, sync::{Arc, Weak}};

use super::{Frame, LightSpot};
use crate::{
    Renderer, Result,
    overlay::{Appearance, Fill},
    raster::Viewport,
    scene::{Backdrop, Group, Shape},
};

/// Ordered operations over a premultiplied sRGB backdrop.
#[derive(Clone, Copy)]
pub enum FrameStage<'a> {
    Glass { backdrop: Backdrop, groups: &'a [Group<'a>] },
    Interaction {
        shape: Shape<'a>,
        appearance: Appearance,
        fill: Fill,
        light: Option<LightSpot<'a>>,
    },
}

impl FrameStage<'_> {
    fn draw(self, renderer: &mut Renderer, frame: &mut Frame) -> Result<()> {
        match self {
            Self::Glass { backdrop, groups } => frame.render_groups(renderer,backdrop,groups),
            Self::Interaction { shape, appearance, fill, light } =>
                frame.interact(renderer,shape,appearance,fill,light),
        }
    }

    fn encode(self, key: &mut Key<'_>) -> Option<()> {
        match self {
            Self::Glass { backdrop, groups } => {
                key.bytes(&[0,backdrop as u8])?;
                key.count(groups.len())?;
                for group in groups {
                    key.bytes(&[group.material.kind as u8])?;
                    key.count(group.material.uniforms.len())?;
                    key.bytes(group.material.uniforms)?;
                    key.count(group.material.extension.len())?;
                    key.bytes(group.material.extension)?;
                    for value in [group.opacity.to_bits(),group.smoothness.to_bits(),group.blur_levels] {
                        key.bytes(&value.to_ne_bytes())?;
                    }
                    key.count(group.shapes.len())?;
                    for &shape in group.shapes { key.shape(shape)?; }
                }
            }
            Self::Interaction {shape,appearance,fill,light} => {
                key.bytes(&[1,appearance as u8,fill as u8,u8::from(light.is_some())])?;
                key.shape(shape)?;
                if let Some(light) = light {
                    let glow = light.glow;
                    for value in [glow.opacity,glow.big_opacity,glow.scale,glow.diameter]
                        .into_iter().chain(glow.position).chain(light.origin)
                    {
                        key.bytes(&value.to_bits().to_ne_bytes())?;
                    }
                }
            }
        }
        Some(())
    }
}

struct Key<'a> {
    data: Vec<u8>,
    expected: Option<&'a [u8]>,
    written: usize,
    limit: usize,
}

impl Key<'_> {
    fn bytes(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.written.checked_add(bytes.len())?;
        if end>self.limit { return None; }
        if let Some(expected) = self.expected {
            if &expected[self.written..end]!=bytes { return None; }
        } else {
            self.data.extend_from_slice(bytes);
        }
        self.written=end;
        Some(())
    }

    fn count(&mut self, length: usize) -> Option<()> {
        self.bytes(&length.to_ne_bytes())
    }

    fn shape(&mut self, shape: Shape<'_>) -> Option<()> {
        match shape {
            Shape::Field(raster) => {
                self.bytes(&[0])?;
                self.bytes(&raster.size.width.to_ne_bytes())?;
                self.bytes(&raster.size.height.to_ne_bytes())?;
                self.count(raster.rgba.len())?;
                if raster.rgba.len()>self.limit.saturating_sub(self.written)/4 {
                    return None;
                }
                for value in raster.rgba { self.bytes(&value.to_bits().to_ne_bytes())?; }
            }
            Shape::Geometry { values, transform } => {
                self.bytes(&[1])?;
                for value in values.into_iter().flatten().chain(transform.into_iter().flatten()) {
                    self.bytes(&value.to_bits().to_ne_bytes())?;
                }
            }
        }
        Some(())
    }
}

struct Scene {
    viewport: Viewport,
    source: Weak<Vec<[f32;4]>>,
    renderer: rc::Weak<()>,
    keys: Vec<Box<[u8]>>,
    /// Outputs after the first N stages. Keep the final image separately.
    prefixes: Vec<Frame>,
    final_frame: Frame,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub reused_stages: u64,
    pub resident_bytes: usize,
}

/// Cached output for a single scene with a caller-specified resident byte limit.
/// The final raster and its keys take priority over intermediate prefixes.
/// Cached rasters are CPU-side and scoped to the renderer that produced them.
pub struct FrameCache {
    budget: usize,
    latest: Option<Scene>,
    stats: CacheStats,
}

impl FrameCache {
    pub fn new(resident_budget_bytes: usize) -> Self {
        Self { budget: resident_budget_bytes, latest: None, stats: CacheStats::default() }
    }

    pub fn stats(&self) -> CacheStats { self.stats }

    /// Execute a scene when its source or stage inputs change.
    ///
    /// Clones of an unchanged backdrop preserve its identity. A new backdrop
    /// raster invalidates the full frame, including all downstream effects.
    /// Versioned rectangle updates with tighter damage use `scene::LiveScene`.
    /// A failed render preserves the previously committed scene.
    pub fn render(&mut self, renderer: &mut Renderer, source: &Frame, stages: &[FrameStage<'_>]) -> Result<Frame> {
        source.check_target(renderer)?;
        let viewport = source.viewport;
        let source_identity = Arc::downgrade(&source.pixels);
        let renderer_identity = rc::Rc::downgrade(&renderer.identity);
        let matching = self.latest.as_ref().filter(|scene| scene.viewport==viewport &&
            Weak::ptr_eq(&scene.source,&source_identity) &&
            rc::Weak::ptr_eq(&scene.renderer,&renderer_identity));
        if let Some(scene) = matching && scene.keys.len()==stages.len()
            && stages.iter().zip(&scene.keys).all(|(stage,expected)| {
                let mut key = Key { data:Vec::new(), expected:Some(expected), written:0, limit:expected.len() };
                stage.encode(&mut key).is_some() && key.written==expected.len()
            })
        {
            self.stats.hits += 1;
            return Ok(scene.final_frame.clone());
        }

        let frame_bytes = viewport.pixels().pixels()*16;
        let mut key_budget = self.budget.saturating_sub(frame_bytes);
        let mut keys = Vec::new();
        for stage in stages {
            let mut key = Key {
                data:Vec::new(), expected:None, written:0,
                limit:key_budget.saturating_sub(size_of::<Box<[u8]>>()),
            };
            if stage.encode(&mut key).is_none() { break; }
            key_budget -= key.data.len()+size_of::<Box<[u8]>>();
            keys.push(key.data.into_boxed_slice());
        }
        let cacheable = keys.len()==stages.len() && self.budget>=frame_bytes;

        let prefix_budget = key_budget/(frame_bytes+size_of::<Frame>());
        let prefix_limit = (stages.len().saturating_sub(1)).min(prefix_budget);
        let mut prefixes = Vec::new();
        let mut frame = source.clone();
        let reused = if let Some(scene) = matching {
            let count = scene.keys.iter().zip(&keys).take_while(|(a,b)| a==b).count();
            let usable = count.min(scene.prefixes.len());
            prefixes.extend(scene.prefixes[..usable.min(prefix_limit)].iter().cloned());
            if usable>0 {
                frame = scene.prefixes[usable-1].clone();
            }
            usable
        } else { 0 };
        for (index,stage) in stages.iter().enumerate().skip(reused) {
            stage.draw(renderer,&mut frame)?;
            if cacheable && index<prefix_limit { prefixes.push(frame.clone()); }
        }

        self.stats.reused_stages += reused as u64;
        self.stats.misses += 1;
        if cacheable {
            self.stats.resident_bytes = self.budget-key_budget+prefixes.len()*(frame_bytes+size_of::<Frame>());
            self.latest = Some(Scene {
                viewport, source:source_identity, renderer:renderer_identity,
                keys, prefixes, final_frame:frame.clone(),
            });
        } else {
            self.stats.resident_bytes = 0;
            self.latest = None;
        }
        Ok(frame)
    }
}
