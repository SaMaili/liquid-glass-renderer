//! Local worker for the browser demo. The HTTP host supplies scene geometry.
use liquid_glass_core::{
    Renderer,
    frame::{Frame, LightSpot},
    interaction::Glow,
    material::{BackgroundStyle, recipe::{RecipeRole, SdrRecipe}, spec::Dimensions},
    overlay::{Appearance, Fill},
    raster::{Scale, Viewport},
    scene::{Backdrop, Group, MaterialKind, Shape},
};
use std::io::{self, BufReader, BufWriter, Read, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn read<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn floats<const N: usize>(input: &mut impl Read) -> io::Result<[f32; N]> {
    let mut result = [0.0; N];
    for value in &mut result {
        *value = f32::from_le_bytes(read(input)?);
    }
    Ok(result)
}

fn shape(input: &mut impl Read) -> io::Result<Shape<'static>> {
    Ok(Shape::Geometry {
        values: [floats(input)?, floats(input)?, floats(input)?],
        transform: [floats(input)?, floats(input)?],
    })
}

fn appearance(value: u8) -> Result<Appearance> {
    match value {
        0 => Ok(Appearance::Light),
        1 => Ok(Appearance::Dark),
        _ => Err("invalid appearance".into()),
    }
}

fn render(frame: &mut Frame, renderer: &mut Renderer, input: &mut impl Read) -> Result<()> {
    let [count] = read(input)?;
    if !(1..=8).contains(&count) {
        return Err("invalid group count".into());
    }
    let mut groups = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let [shape_count, role, theme, flags] = read(input)?;
        let role = match role {
            0 => RecipeRole::Dock,
            1 => RecipeRole::Regular,
            2 => RecipeRole::Clear,
            _ => return Err("invalid material role".into()),
        };
        let [height, diffusion, r, g, b, a, fill, luminance, smoothness, opacity] =
            floats(input)?;
        let refraction_gain = f64::from_le_bytes(read(input)?);
        if !(1..=8).contains(&shape_count) || flags & !3 != 0 {
            return Err("invalid shape count or material flags".into());
        }
        let recipe = SdrRecipe {
            role,
            appearance: appearance(theme)?,
            dimensions: Dimensions {
                minimum: Some(f64::from(height)),
                largest_area_minimum: Some(f64::from(height)),
            },
            diffusion,
            tint: Some([r, g, b, a]),
            style: BackgroundStyle {
                refraction_gain,
                key_fill_amount: (flags & 1 != 0).then_some(fill),
            },
        };
        let material = recipe.build_with_luminance(frame.viewport(), (flags & 2 != 0).then_some(luminance))?;
        let shapes = (0..shape_count).map(|_| shape(input)).collect::<io::Result<Vec<_>>>()?;
        groups.push((shapes, material, smoothness, opacity));
    }
    let borrowed = groups.iter().map(|(shapes, prepared, smoothness, opacity)| Ok(Group {
        shapes,
        material: prepared.material.material(MaterialKind::All)?,
        smoothness: *smoothness,
        opacity: *opacity,
        blur_levels: prepared.blur_levels,
    })).collect::<Result<Vec<_>>>()?;
    frame.render_groups(renderer, Backdrop::Original, &borrowed)?;
    Ok(())
}

fn overlay(frame: &mut Frame, renderer: &mut Renderer, input: &mut impl Read) -> Result<()> {
    let [theme, fill, has_glow] = read(input)?;
    let fill = match fill {
        0 => Fill::Idle,
        1 => Fill::Rollover,
        2 => Fill::Pressed,
        _ => return Err("invalid interaction role".into()),
    };
    let shape = shape(input)?;
    let mut spot = None;
    let glow;
    if has_glow != 0 {
        let mut values = [0.0; 8];
        for value in &mut values {
            *value = f64::from_le_bytes(read(input)?);
        }
        let [opacity, big_opacity, scale, diameter, x, y, ox, oy] = values;
        glow = Glow { opacity, big_opacity, scale, diameter, position: [x, y] };
        spot = Some(LightSpot { glow: &glow, origin: [ox, oy] });
    }
    frame.interact(renderer, shape, appearance(theme)?, fill, spot)?;
    Ok(())
}

fn main() -> Result<()> {
    let scale = match std::env::args().nth(1).as_deref() {
        Some("1") => Scale::One,
        Some("2") => Scale::Two,
        _ => return Err("expected demo scale 1 or 2".into()),
    };
    let viewport = Viewport::new(840, 480, scale)?;
    let mut renderer = Renderer::new(viewport.pixels())?;
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    let mut raw = vec![0; viewport.pixels().pixels() * 16];
    let mut frame = None;
    output.write_all(b"LGD1")?;
    output.flush()?;
    loop {
        match read::<1>(&mut input)?[0] {
            0 => return Ok(()),
            1 => {
                input.read_exact(&mut raw)?;
                let pixels = raw.as_chunks::<16>().0.iter().map(|pixel| {
                    let (channels, _) = pixel.as_chunks::<4>();
                    std::array::from_fn(|i| f32::from_le_bytes(channels[i]))
                }).collect();
                frame = Some(Frame::new(viewport, pixels)?);
            }
            command => {
                let frame = frame.as_mut().ok_or("demo frame has not started")?;
                match command {
                    2 => render(frame, &mut renderer, &mut input)?,
                    3 => overlay(frame, &mut renderer, &mut input)?,
                    4 => {
                        for (pixel, buffer) in frame.pixels().iter().zip(raw.as_chunks_mut::<16>().0) {
                            let (channels, _) = buffer.as_chunks_mut::<4>();
                            for (value, bytes) in pixel.iter().zip(channels) {
                                *bytes = value.to_le_bytes();
                            }
                        }
                        output.write_all(&raw)?;
                        output.flush()?;
                    }
                    _ => return Err("invalid demo command".into()),
                }
            }
        }
    }
}
