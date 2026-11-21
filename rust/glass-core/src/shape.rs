//! Rectangle packing from emulated 26A434 QuartzCore instructions.
use crate::{Result, raster::Viewport, scene::Shape};

/// Layout rectangle in logical points. Radius order follows the emitter slots.
#[derive(Clone, Copy)]
pub struct Rectangle {
    pub origin: [f64; 2],
    pub size: [f64; 2],
    pub radii: [f64; 4],
    pub continuous: bool,
}

/// Forward affine map with columns [a,b], [c,d] and translation [tx,ty].
/// Distances remain in logical points at both supported raster scales.
pub fn affine_rectangle(
    viewport: Viewport,
    rectangle: Rectangle,
    affine: [f64; 6],
) -> Result<Shape<'static>> {
    let points = viewport.points();
    affine_rectangle_in([f64::from(points.width), f64::from(points.height)], rectangle, affine)
}

/// Pack a rectangle in a raster whose logical extent may be fractional.
pub fn affine_rectangle_in(
    logical_extent: [f64; 2],
    rectangle: Rectangle,
    affine: [f64; 6],
) -> Result<Shape<'static>> {
    if !logical_extent.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err("positive finite logical raster extent required".into());
    }
    if !rectangle.origin.iter().chain(&affine).all(|v| v.is_finite())
        || !rectangle.size.iter().all(|v| v.is_finite() && *v > 0.0)
        || !rectangle.radii.iter().all(|v| v.is_finite() && v.abs() <= 1.0e6)
    {
        return Err("finite affine, positive size and bounded corner radii required".into());
    }

    // Float32 conversion and fused products mirror affine_metric at 0x18b2a2fb8.
    let [a, b, c, d, _, _] = affine.map(|v| v as f32);
    let determinant = d.mul_add(a, -(c * b));
    let row_a = c.mul_add(c, a * a).sqrt();
    let row_b = d.mul_add(d, b * b).sqrt();
    let inverse_reach = 1.0 / row_a.max(row_b);
    let metric = determinant * inverse_reach;
    if !(metric > 0.0 && metric <= 1.0e6) {
        return Err("unsupported native affine metric".into());
    }
    let normal = [d * inverse_reach, -(b * inverse_reach), -(c * inverse_reach), a * inverse_reach];
    let radii = rectangle.radii.map(|r| (r as f32).max(0.01));
    let extent = rectangle.size.map(|v| (v * f64::from(metric)) as f32 * 0.5);
    let equal = radii.iter().all(|&r| r == radii[0]);
    let (mode, packed_radii) = if equal {
        let radius = radii[0] * metric;
        let blend = if rectangle.continuous {
            let negative = f64::from_bits(0xbff7_21e8_a7a4_b61b);
            let positive = f64::from_bits(0x4007_21e8_a7a4_b61b);
            let maximum = f64::from(radius) * 1.528665;
            extent.map(|e| (f64::from(e * 2.0) * negative / maximum + positive).clamp(0.0, 1.0) as f32)
        } else {
            [1.0; 2]
        };
        (4.0, [blend[0], blend[1], radius, 0.0])
    } else {
        let sign = if rectangle.continuous { -1.0 } else { 1.0 };
        (5.0, radii.map(|r| r * metric * sign))
    };
    let values = [[extent[0], extent[1], mode, 0.0], normal, packed_radii];

    let [cx, cy] = std::array::from_fn(|i| rectangle.origin[i] + rectangle.size[i] / 2.0);
    let [width, height] = logical_extent;
    let [p, q, r, s] = normal.map(f64::from);
    let [_, _, _, _, tx, ty] = affine;
    let metric = f64::from(metric);
    let transform = [
        [(p * width) as f32, (r * height) as f32,
            (-(p * tx + r * ty) - metric * cx) as f32],
        [(q * width) as f32, (s * height) as f32,
            (-(q * tx + s * ty) - metric * cy) as f32],
    ];
    if !values.iter().flatten().chain(transform.iter().flatten()).all(|v| v.is_finite()) {
        return Err("affine rectangle overflows GPU geometry".into());
    }
    Ok(Shape::Geometry { values, transform })
}
