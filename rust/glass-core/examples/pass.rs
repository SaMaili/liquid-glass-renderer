//! Two overlapping foreground glass groups over changing content.
use liquid_glass_core::{
    Raster, Renderer,
    scene::{Backdrop, Group, LiveScene, Material, MaterialKind, Shape},
    raster::{Content, Patch, PixelRect, Scale, Viewport},
};
use std::{fs::File, io::{BufWriter, Write}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "glass-pass.ppm".into());
    let scale = match args.next().as_deref() {
        None | Some("1") => Scale::One,
        Some("2") => Scale::Two,
        _ => return Err("scale must be 1 or 2".into()),
    };
    let viewport = Viewport::new(256, 192, scale)?;
    let size = viewport.pixels();
    let mut source = vec![0.0; size.pixels() * 4];
    let mut fields = [source.clone(), source.clone()];
    for y in 0..size.height {
        for x in 0..size.width {
            let i = (y * size.width + x) as usize * 4;
            let [px, py] = viewport.pixel_center(x, y).unwrap().map(|v| v as f32);
            source[i..i + 4].copy_from_slice(&[px / 256.0, py / 192.0, 0.35, 1.0]);
            for (field, center_x) in fields.iter_mut().zip([100.0, 156.0]) {
                let dx = px - center_x;
                let dy = py - 96.0;
                let length = dx.hypot(dy).max(1.0e-6);
                field[i..i + 4].copy_from_slice(&[length - 62.0, dx / length, dy / length, 1.0]);
            }
        }
    }
    let content = Content::new(viewport, 1, source)?;
    let mut uniforms = [0u8; 80];
    // The distance field and material displacement use logical points. UV
    // conversion therefore uses the point dimensions at both raster scales.
    for (offset, values) in [
        (0, &[1.0 / 256.0, 0.0, 0.0, 1.0 / 192.0][..]),
        (16, &[8.0, 0.1, 0.0, 4.0, 0.1, 0.0]),
        (40, &[1.0, 0.0]),
        (48, &[0.0, 1.0, 0.0, 0.0]),
        (64, &[1.0, 0.0]),
    ] {
        for (i, &value) in values.iter().enumerate() {
            uniforms[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&f32::to_ne_bytes(value));
        }
    }

    let mut renderer = Renderer::new(size)?;
    let shapes = fields.each_ref().map(|rgba| [Shape::Field(Raster { size, rgba })]);
    let groups = shapes.each_ref().map(|shapes| Group {
        shapes,
        shape_tags: &[],
        tints: &[],
        material: Material { kind: MaterialKind::Foreground, uniforms: &uniforms, extension: &[] },
        opacity: 1.0,
        smoothness: 0.0,
        blur_levels: 1,
    });
    let mut scene = LiveScene::new(&mut renderer, content, Backdrop::Current, &groups, 32*1024*1024)?;
    let earlier = scene.pixels().to_vec();
    let factor = scale.factor();
    let rect = PixelRect { x: 122 * factor, y: 90 * factor, width: 12 * factor, height: 12 * factor };
    let patch = [0.95, 0.65, 0.10, 1.0].repeat((rect.width * rect.height) as usize);
    let rim = PixelRect { x: 124 * factor, y: 146 * factor, width: 8 * factor, height: 8 * factor };
    let rim_patch = [0.1, 0.1, 0.95, 1.0].repeat((rim.width * rim.height) as usize);
    let damage = scene.update(&mut renderer, 2, &[Patch { rect, rgba: &patch }, Patch { rect: rim, rgba: &rim_patch }])?;
    let image = scene.pixels();
    let center = (size.height as usize / 2 * size.width as usize + size.width as usize / 2) * 4;
    assert!((image[center] - earlier[center]).abs() > 0.25);
    let far = (10 * factor * size.width + 10 * factor) as usize * 4;
    assert_eq!(image[far..far + 4], earlier[far..far + 4]);
    let outside_patch = |x, y, r: PixelRect| {
        x < r.x || x >= r.x + r.width || y < r.y || y >= r.y + r.height
    };
    let changed_beyond_rim = image.chunks_exact(4).zip(earlier.chunks_exact(4))
        .enumerate().any(|(i, (new, old))| {
            let x = i as u32 % size.width;
            let y = i as u32 / size.width;
            outside_patch(x, y, rect) && outside_patch(x, y, rim)
                && (new[2] - old[2]).abs() > 0.01
        });
    assert!(changed_beyond_rim, "refraction must carry content changes beyond the input rectangle");

    let mut out = BufWriter::new(File::create(&path)?);
    write!(out, "P6\n{} {}\n255\n", size.width, size.height)?;
    for pixel in image.chunks_exact(4) {
        let rgb = [pixel[0], pixel[1], pixel[2]].map(|value| {
            let linear = value.clamp(0.0, 1.0);
            let encoded = if linear <= 0.0031308 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        });
        out.write_all(&rgb)?;
    }
    out.flush()?;
    println!("{path} at {}× on {} {:?} damage {:?}", factor, renderer.device()?, renderer.stats(), damage);
    Ok(())
}
