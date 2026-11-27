//! SDR widget regions on compositor-owned rasters, including fractional scale.
use liquid_glass_core::{
    Renderer, Result, Size,
    frame::Frame,
    material::{BackgroundStyle, recipe::{KeyFillHighlight, RecipeRole, SdrRecipe}, spec::Dimensions},
    overlay::Appearance,
    raster::{Scale, Viewport},
    scene::{Backdrop, Group, MaterialKind, Shape},
    shape::{Rectangle, affine_rectangle_in},
};
use std::collections::VecDeque;

const CACHED_SIZES: usize = 4;
const MAX_CROP: u32 = 4096;
const MAX_CROP_PIXELS: usize = 9_000_000;

#[derive(Default)]
pub struct WidgetRenderer {
    renderers: VecDeque<Renderer>,
    reported_error: bool,
}

/// Crop and shape coordinates are in physical pixels. Radius is in points.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Widget {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub bounds: [f32; 4],
    pub radius: f32,
    pub diffusion: f32,
    pub role: u32,
    pub appearance: u32,
    pub opacity: f32,
    pub tint: [f32; 4],
    pub pointer: f32,
    pub tint2: [f32; 4],
    pub tint_stops: [f32; 2],
}

#[repr(C)]
pub struct Prepared {
    geometry: [[f32; 4]; 6],
    transform: [[f32; 4]; 4],
    uniforms: [u8; 320],
    extension: [u8; 32],
    levels: u32,
    widget_plate: [[f32; 4]; 3],
    widget_tint: [f32; 4],
    widget_tint2: [f32; 4],
    widget_tint_axis: [f32; 4],
    legibility_lod: f32,
    highlight_matrix: [[f32; 4]; 4],
    highlight_lobes: [[f32; 4]; 2],
}

/// Measured center response on Apple home-screen clock/weather widgets.
/// In encoded sRGB, out = p * input + q * Rec.709 luma(input) + k.
fn widget_plate(appearance: u32) -> [[f32; 4]; 3] {
    let (gray, blue, offset) = if appearance == 1 {
        ([45.02, 45.02, 45.02], [2.05, 2.01, 198.02], 12.10)
    } else {
        ([117.08, 117.08, 117.06], [21.01, 21.01, 245.0], 30.05)
    };
    std::array::from_fn(|i| {
        let luma_gain = (gray[i] - offset) / 128.0;
        let on_blue = (blue[i] - offset) / 245.0;
        let q = if i == 2 { (luma_gain - on_blue) / (1.0 - 0.0722) } else { on_blue / 0.0722 };
        [luma_gain - q, q, offset / 255.0, 0.0]
    })
}

/// AGX2 one-axis variance is about 0.94 at each output mip's unit spacing.
/// Include the previous level's bilinear footprint and interpolate variance
/// across mip levels rather than interpolating the logarithm of sigma.
fn legibility_mip(size: Size, scale: f32, amount: f32) -> (f32, u32) {
    let variance = ((1.05 + (15.5 - 1.05) * amount) * scale).powi(2);
    let mut previous = 0.0;
    let mut mip = 1;
    while ((size.width.max(size.height) >> mip) > 0) && mip < 12 {
        let spacing = (1u32 << mip) as f32;
        let next = previous + (0.94 + 0.0625) * spacing * spacing;
        if variance <= next {
            return ((mip - 1) as f32 + (variance - previous) / (next - previous), mip + 1);
        }
        previous = next;
        mip += 1;
    }
    ((mip - 1) as f32, mip)
}

fn prepare(widget: Widget) -> Result<(Size, liquid_glass_core::scene::Shape<'static>, liquid_glass_core::material::recipe::PreparedSdrMaterial)> {
        let size = Size::new(widget.width, widget.height)?;
        if size.width > MAX_CROP || size.height > MAX_CROP || size.pixels() > MAX_CROP_PIXELS
            || !widget.scale.is_finite() || widget.scale <= 0.0
            || !widget.bounds.iter().all(|value| value.is_finite())
            || widget.bounds[2] <= 0.0 || widget.bounds[3] <= 0.0
            || !widget.radius.is_finite() || widget.radius < 0.0
            || !(0.0..=1.0).contains(&widget.opacity)
            || !(0.0..=1.0).contains(&widget.diffusion)
            || !widget.tint.iter().all(|v| (0.0..=1.0).contains(v))
        {
            return Err("invalid widget geometry or SDR raster".into());
        }
        let role = match widget.role {
            0 | 2 => RecipeRole::Regular,
            1 | 6 => RecipeRole::Clear,
            3 => RecipeRole::Dock,
            4 | 5 => RecipeRole::Popover,
            7 => RecipeRole::AppIcons,
            _ => return Err("invalid widget material".into()),
        };
        let appearance = match widget.appearance {
            0 => Appearance::Light,
            1 => Appearance::Dark,
            _ => return Err("invalid widget appearance".into()),
        };
        let scale = f64::from(widget.scale);
        let [x, y, width, height] = widget.bounds.map(|value| f64::from(value) / scale);
        let height = if widget.role == 5 { height - 8.0 } else { height };
        let radius = if widget.role == 3 {
            // The Dock request carries the logical tile size in this field.
            match widget.radius as u32 {
                0..=31 => 12.0,
                32..=47 => 15.0,
                48..=63 => 22.0,
                64..=127 => 28.0,
                _ => 48.0,
            }
        } else {
            f64::from(widget.radius)
        }.min(width.min(height) / 2.0);
        let shape = affine_rectangle_in(
            [f64::from(size.width) / scale, f64::from(size.height) / scale],
            Rectangle { origin: [x, y], size: [width, height], radii: [radius; 4], continuous: true },
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        )?;
        let minimum = if widget.role == 3 {
            height
        } else if widget.role == 6 {
            // Original macOS 26A434 Regular system chrome (bars, sidebars, panels)
            // ramps refraction, edge bleed and blur over [64, 160] pt. Use at least
            // 120 pt so Caelestia's bar, border and panels get the full Regular
            // optics even when the bar is auto-hidden.
            f64::from(widget.radius).clamp(120.0, 160.0)
        } else {
            width.min(height)
        };
        let recipe = SdrRecipe {
            role, appearance,
            dimensions: Dimensions { minimum: Some(minimum), largest_area_minimum: Some(minimum) },
            diffusion: if widget.role == 2 { 0.5 } else { widget.diffusion },
            tint: (widget.role != 2 && widget.tint[3] != 0.0).then_some(widget.tint),
            style: BackgroundStyle::default(),
        }.build_for_raster(size, widget.scale, None)?;
        Ok((size, shape, recipe))
}

// Dock.app 26B123's small cartouche uses a 16 pt square rotated 45°,
// 2.5 pt rounding and one smooth union with the label body.
fn cartouche_tip(widget: Widget, size: Size) -> Result<Option<Shape<'static>>> {
    if widget.role != 5 {
        return Ok(None);
    }
    if !(0.0..=1.0).contains(&widget.pointer) {
        return Err("invalid cartouche pointer".into());
    }
    let scale = f64::from(widget.scale);
    let [x, y, w, h] = widget.bounds.map(|pixel| f64::from(pixel) / scale);
    let edge = (w / 2.0).min(11.0);
    let center = x + (w * f64::from(widget.pointer)).clamp(edge, w - edge);
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let shape = affine_rectangle_in(
        [f64::from(size.width) / scale, f64::from(size.height) / scale],
        Rectangle { origin: [-8.0; 2], size: [16.0; 2], radii: [2.5; 4], continuous: true },
        [diagonal, diagonal, -diagonal, diagonal, center, y + h - 11.0],
    )?;
    Ok(Some(shape))
}

/// # Safety
/// Both pointers must refer to valid, non-overlapping objects for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lg_widget_prepare(widget: *const Widget, output: *mut Prepared) -> bool {
    if widget.is_null() || output.is_null() {
        return false;
    }
    let widget = unsafe { *widget };
    let Ok((size, liquid_glass_core::scene::Shape::Geometry { values, transform }, recipe)) =
        prepare(widget) else { return false; };
    let Ok(tip) = cartouche_tip(widget, size) else { return false; };
    let Ok(material) = recipe.material.material(MaterialKind::All) else { return false; };
    let mut geometry = [[0.0; 4]; 6];
    let mut transforms = [[0.0; 4]; 4];
    geometry[..3].copy_from_slice(&values);
    transforms[..2].copy_from_slice(&transform.map(|[a, b, c]| [a, b, c, 0.0]));
    if let Some(Shape::Geometry { values, transform }) = tip {
        geometry[3..].copy_from_slice(&values);
        transforms[2..].copy_from_slice(&transform.map(|[a, b, c]| [a, b, c, 0.0]));
    }
    let mut extension = [0; 32];
    extension[..material.extension.len()].copy_from_slice(material.extension);
    let measured = widget.role == 2;
    let (legibility_lod, levels) = if measured {
        let (lod, levels) = legibility_mip(size, widget.scale, widget.diffusion);
        (lod, levels.max(recipe.blur_levels))
    } else {
        (-1.0, recipe.blur_levels)
    };
    let role = match widget.role {
        0 | 2 => RecipeRole::Regular,
        1 | 6 => RecipeRole::Clear,
        3 => RecipeRole::Dock,
        4 | 5 => RecipeRole::Popover,
        7 => RecipeRole::AppIcons,
        _ => return false,
    };
    let appearance = match widget.appearance {
        0 => Appearance::Light,
        1 => Appearance::Dark,
        _ => return false,
    };
    let Ok(highlight) = KeyFillHighlight::for_recipe(role, appearance) else { return false; };
    let widget_tint2 = if widget.tint2[3] >= 0.0 && widget.tint2.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
        widget.tint2
    } else {
        widget.tint
    };
    let stop_start = if widget.tint_stops[0].is_finite() { widget.tint_stops[0].clamp(0.0, 1.0) } else { 0.0 };
    let stop_end = if widget.tint_stops[1].is_finite() { widget.tint_stops[1].clamp(0.0, 1.0) } else { 1.0 };
    let widget_tint_axis = [widget.bounds[1], widget.bounds[3].max(1.0), stop_start, stop_end.max(stop_start + 0.001)];
    unsafe { output.write(Prepared {
        geometry,
        transform: transforms,
        uniforms: material.uniforms.try_into().unwrap(),
        extension,
        levels,
        widget_plate: if measured { widget_plate(widget.appearance) } else { [[0.0; 4]; 3] },
        widget_tint: widget.tint,
        widget_tint2,
        widget_tint_axis,
        legibility_lod,
        highlight_matrix: highlight.matrix,
        highlight_lobes: highlight.lobes,
    }) };
    true
}

impl WidgetRenderer {
    pub fn render(&mut self, widget: Widget, rgba: &mut [u8]) -> Result<()> {
        if widget.role == 2 {
            return Err("measured widget material requires the compositor shader".into());
        }
        let (size, shape, recipe) = prepare(widget)?;
        if rgba.len() != size.pixels() * 4 {
            return Err("invalid widget raster length".into());
        }
        let pixels = rgba.as_chunks::<4>().0.iter().map(|&[r,g,b,_]| {
            [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 1.0]
        }).collect();
        // Frame uses the pixel viewport for storage only. Geometry and material
        // use the compositor scale explicitly as there is no interaction pass.
        let mut frame = Frame::new(Viewport::new(size.width, size.height, Scale::One)?, pixels)?;
        if let Some(index) = self.renderers.iter().position(|renderer| renderer.size() == size) {
            let renderer = self.renderers.remove(index).unwrap();
            self.renderers.push_front(renderer);
        } else {
            if self.renderers.len() == CACHED_SIZES {
                self.renderers.pop_back();
            }
            // Four RGBA32F images fit including the source's mip chain.
            self.renderers.push_front(Renderer::with_budget(size, size.pixels() * 64)?);
        }
        let tip = cartouche_tip(widget, size)?;
        let shapes = [shape, tip.unwrap_or(shape)];
        frame.render_groups(self.renderers.front_mut().unwrap(), Backdrop::Original, &[Group {
            shapes: &shapes[..if tip.is_some() { 2 } else { 1 }],
            shape_tags: &[],
            tints: &[],
            material: recipe.material.material(MaterialKind::All)?,
            blur_levels: recipe.blur_levels,
            opacity: widget.opacity,
            smoothness: if tip.is_some() { 25.0 } else { 0.0 },
        }])?;
        for (destination, source) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(frame.pixels()) {
            *destination = source.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8);
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lg_widget_renderer_create() -> *mut WidgetRenderer {
    Box::into_raw(Box::new(WidgetRenderer::default()))
}

/// # Safety
/// Non-null renderer must be live and owned by the calling GL thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lg_widget_renderer_destroy(renderer: *mut WidgetRenderer) {
    if !renderer.is_null() {
        drop(unsafe { Box::from_raw(renderer) });
    }
}

/// # Safety
/// Renderer must be live on the calling GL thread. Widget must be readable
/// and pixels must be exclusive and writable for the given nonzero length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lg_widget_render(
    renderer: *mut WidgetRenderer,
    widget: *const Widget,
    pixels: *mut u8,
    length: usize,
) -> bool {
    if renderer.is_null() || widget.is_null() || pixels.is_null()
        || length == 0 || length > MAX_CROP_PIXELS * 4
    {
        return false;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let widget = unsafe { *widget };
        let pixels = unsafe { std::slice::from_raw_parts_mut(pixels, length) };
        let renderer = unsafe { &mut *renderer };
        match renderer.render(widget, pixels) {
            Ok(()) => true,
            Err(error) => {
                if !renderer.reported_error {
                    eprintln!("Liquid Glass widget: {error}");
                    renderer.reported_error = true;
                }
                false
            }
        }
    })).unwrap_or(false)
}
