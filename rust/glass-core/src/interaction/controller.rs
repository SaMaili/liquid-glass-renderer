//! Pointer and animation state. The caller supplies the frame timestamp.
use super::{Gesture, Glow, Input, Profile, Spring, valid_time};
use crate::{Result, overlay::Fill, raster::Viewport, scene::Shape, shape::{self, Rectangle}};

#[derive(Debug, PartialEq)]
pub struct Presentation {
    pub scale: [f64; 2],
    pub translation: [f64; 2],
    pub glow: Glow,
    pub needs_frame: bool,
}

impl Presentation {
    /// Apply the original flex presentation about the layout rectangle's center.
    pub fn rectangle(&self, viewport: Viewport, rectangle: Rectangle) -> Result<Shape<'static>> {
        let center: [f64; 2] = std::array::from_fn(|i| rectangle.origin[i] + rectangle.size[i] / 2.0);
        let [sx, sy] = self.scale;
        let tx = (1.0 - sx) * center[0] + self.translation[0];
        let ty = (1.0 - sy) * center[1] + self.translation[1];
        shape::affine_rectangle(viewport, rectangle, [sx, 0.0, 0.0, sy, tx, ty])
    }
}

/// Stationary-surface interaction controller following the demo's event policy.
/// Geometry deformation and glow targets use the separately verified kernels.
#[derive(Clone, Copy)]
pub struct Controller {
    gesture: Gesture,
    flex: [Spring; 4],
    glow: [Spring; 5],
    glow_translation: [f64; 2],
    reduced_motion: bool,
    reset_glow_clock: bool,
    sleeping: bool,
    hovered: bool,
    pending: bool,
    last_time: f64,
}

impl Controller {
    pub fn new(size: [f64; 2]) -> Result<Self> {
        Profile::automatic(size, Input::Cursor)?;
        let start = size.map(|v| v / 2.0);
        Ok(Self {
            gesture: Gesture {
                size, start, translation: [0.0; 2], input: Input::Cursor, active: false,
                lift: true, flex: true, reduce_highlighting: false,
            },
            flex: [1.0, 1.0, 0.0, 0.0].map(at_zero),
            glow: [0.0, 0.0, 1.0, start[0], start[1]].map(at_zero),
            glow_translation: [0.0; 2],
            reduced_motion: false,
            reset_glow_clock: false,
            sleeping: true,
            hovered: false,
            pending: false,
            last_time: 0.0,
        })
    }

    pub fn needs_frame(&self) -> bool { self.pending }
    pub fn is_pressed(&self) -> bool { self.gesture.active }

    /// The original button fill role for the current interaction.
    pub fn fill(&self) -> Fill {
        if self.gesture.active {
            Fill::Pressed
        } else if self.hovered {
            Fill::Rollover
        } else {
            Fill::Idle
        }
    }

    /// Report whether the cursor is over the control. The caller owns hit testing.
    /// On captured release, update hover from the actual release position.
    pub fn set_hovered(&mut self, hovered: bool) {
        let hovered = hovered && !(self.gesture.active && matches!(self.gesture.input, Input::Touch));
        if self.hovered != hovered {
            self.hovered = hovered;
            // A pressed button keeps the same fill until it is released.
            self.pending |= !self.gesture.active;
        }
    }

    pub fn press(&mut self, input: Input, start: [f64; 2]) -> Result<()> {
        validate_coords(start)?;
        if !self.gesture.active {
            self.glow[2..].copy_from_slice(&[1.0, start[0], start[1]].map(at_zero));
            self.reset_glow_clock = true;
        }
        self.gesture.input = input;
        if matches!(input, Input::Touch) {
            self.hovered = false;
        }
        self.gesture.start = start;
        self.gesture.active = true;
        self.gesture.translation = [0.0; 2];
        self.glow_translation = [0.0; 2];
        self.pending = true;
        Ok(())
    }

    pub fn drag(&mut self, translation: [f64; 2]) -> Result<()> {
        validate_coords(translation)?;
        if self.gesture.active && self.gesture.translation != translation {
            self.gesture.translation = translation;
            self.glow_translation = translation;
            self.pending = true;
        }
        Ok(())
    }

    pub fn release(&mut self) {
        if self.gesture.active {
            self.gesture.active = false;
            self.gesture.translation = [0.0; 2];
            self.pending = true;
        }
    }

    pub fn cancel(&mut self) {
        self.release();
        self.hovered = false;
        self.flex = [1.0, 1.0, 0.0, 0.0].map(at_zero);
        let [x, y] = self.gesture.start;
        self.glow = [0.0, 0.0, 1.0, x, y].map(at_zero);
        self.glow_translation = [0.0; 2];
        self.reset_glow_clock = false;
        self.sleeping = true;
        self.pending = true;
    }

    pub fn resize(&mut self, size: [f64; 2]) -> Result<()> {
        Profile::automatic(size, self.gesture.input)?;
        if size != self.gesture.size {
            let start = std::array::from_fn(|i| self.gesture.start[i] * size[i] / self.gesture.size[i]);
            validate_coords(start)?;
            self.gesture.start = start;
            self.gesture.size = size;
            self.reset_glow_clock = true;
            self.pending = true;
        }
        Ok(())
    }

    pub fn set_reduced_motion(&mut self, value: bool) {
        if self.reduced_motion != value {
            self.reduced_motion = value;
            self.pending = true;
        }
    }

    pub fn set_reduced_highlighting(&mut self, value: bool) {
        if self.gesture.reduce_highlighting != value {
            self.gesture.reduce_highlighting = value;
            self.pending = true;
        }
    }

    /// Return a new presentation only when a frame is due.
    pub fn sample(&mut self, now: f64) -> Result<Option<Presentation>> {
        if !valid_time(now) || now < self.last_time {
            return Err("invalid or nonmonotonic interaction timestamp".into());
        }
        if !self.pending {
            return Ok(None);
        }
        let mut next = *self;
        let presentation = next.advance(now)?;
        *self = next;
        Ok(Some(presentation))
    }

    fn advance(&mut self, now: f64) -> Result<Presentation> {
        if self.sleeping {
            for spring in self.flex.iter_mut().chain(&mut self.glow) {
                retime(spring, now);
            }
        }
        if self.reset_glow_clock {
            for spring in &mut self.glow[2..] {
                retime(spring, now);
            }
            self.reset_glow_clock = false;
        }

        let mut flex = self.gesture.flex_target()?;
        if self.reduced_motion {
            flex.scale = [1.0; 2];
            flex.translation = [0.0; 2];
        }
        let [sx, sy] = flex.scale;
        let [tx, ty] = flex.translation;
        let mut animating = false;
        let mut values = [0.0; 4];
        for ((value, spring), target) in values.iter_mut().zip(&mut self.flex).zip([sx, sy, tx, ty]) {
            (*value, animating) = step(spring, target, now, flex.spring, self.reduced_motion, animating)?;
        }

        let target = Gesture { translation: self.glow_translation, ..self.gesture }.glow_target()?;
        let glow_visible = self.gesture.active || target.big_opacity > 0.0 || target.opacity > 0.0
            || self.glow[..2].iter().any(|s| s.offset > 1.0e-4);
        let immediate = self.reduced_motion || self.gesture.reduce_highlighting || !glow_visible;
        let targets = [target.big_opacity, target.opacity, target.scale, target.position[0], target.position[1]];
        let mut light = [0.0; 5];
        for i in 0..light.len() {
            let response = [0.1, 0.3, 0.3, 0.15, 0.15][i];
            (light[i], animating) = step(&mut self.glow[i], targets[i], now, [response, 1.0], immediate, animating)?;
        }

        self.last_time = now;
        self.pending = animating || self.flex.iter().any(moving)
            || (glow_visible && self.glow.iter().any(moving));
        self.sleeping = !self.pending;
        Ok(Presentation {
            scale: [values[0], values[1]],
            translation: [values[2], values[3]],
            glow: Glow {
                big_opacity: light[0], opacity: light[1], scale: light[2],
                position: [light[3], light[4]], diameter: target.diameter,
            },
            needs_frame: self.pending,
        })
    }
}

fn validate_coords(values: [f64; 2]) -> Result<()> {
    if !values.into_iter().all(f64::is_finite) {
        return Err("finite interaction coordinates required".into());
    }
    Ok(())
}

fn at_zero(offset: f64) -> Spring {
    Spring { offset, velocity: 0.0, force: 0.0, time: 0.0, last_request: 0.0 }
}

fn retime(spring: &mut Spring, now: f64) {
    spring.time = now;
    spring.last_request = now;
}

fn moving(spring: &Spring) -> bool {
    spring.velocity.abs() > 1.0e-4
}

fn step(
    spring: &mut Spring,
    target: f64,
    now: f64,
    [response, damping]: [f64; 2],
    immediate: bool,
    already_animating: bool,
) -> Result<(f64, bool)> {
    if !immediate && let Some(value) = spring.advance(target, now, response, damping)? {
        return Ok((value, true));
    }
    *spring = Spring::new(target, now)?;
    Ok((target, already_animating))
}
