//! Replay chained frame stages from the v8 fixture driver.
mod fixture;

use liquid_glass_core::{
    Renderer,
    frame::{Frame, LightSpot},
    interaction::Glow,
    overlay::{Appearance, Fill},
    raster::{Scale, Viewport},
    scene::{Backdrop, Shape},
};
use std::io::{self, BufReader, BufWriter, Read, Write};

use fixture::{bytes, floats, uint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    match std::env::args().nth(1).as_deref() {
        None | Some("1") => replay(&mut input, &mut output, Scale::One),
        Some("2") => replay(&mut input, &mut output, Scale::Two),
        Some("--sequence") => loop {
            let scale = match uint(&mut input)? {
                0 => break Ok(()),
                1 => Scale::One,
                2 => Scale::Two,
                _ => return Err("expected render scale 1 or 2".into()),
            };
            replay(&mut input, &mut output, scale)?;
        },
        _ => Err("expected render scale 1 or 2".into()),
    }
}

fn replay(
    mut input: &mut impl Read,
    output: &mut impl Write,
    scale: Scale,
) -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Viewport::new(uint(&mut input)?, uint(&mut input)?, scale)?;
    let mut renderer = Renderer::new(viewport.pixels())?;
    let mut raster = vec![0; viewport.pixels().pixels() * 16];
    while let count @ 1..=64 = uint(&mut input)? {
        let straight_output = uint(&mut input)? == 1;
        input.read_exact(&mut raster)?;
        let pixels = raster.chunks_exact(16).map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[4*i..4*(i+1)].try_into().unwrap()))
        }).collect();
        let mut frame = Frame::new(viewport, pixels)?;
        for _ in 0..count {
            match uint(&mut input)? {
                0 => {
                    let groups = uint(&mut input)?;
                    let backdrop = match uint(&mut input)? {
                        0 => Backdrop::Original,
                        1 => Backdrop::Current,
                        _ => return Err("unsupported backdrop mode".into()),
                    };
                    if !(1..=64).contains(&groups) {
                        return Err("unsupported group count".into());
                    }
                    let fixtures = fixture::groups(&mut input, groups)?;
                    frame.render_groups(&mut renderer, backdrop, &fixture::borrow_groups(&fixtures))?;
                }
                1 => {
                    let appearance = match uint(&mut input)? {
                        0 => Appearance::Light,
                        1 => Appearance::Dark,
                        _ => return Err("unsupported appearance".into()),
                    };
                    let fill = match uint(&mut input)? {
                        0 => Fill::Idle,
                        1 => Fill::Rollover,
                        2 => Fill::Pressed,
                        _ => return Err("unsupported fill".into()),
                    };
                    let shape = Shape::Geometry {
                        values: [floats(&mut input)?, floats(&mut input)?, floats(&mut input)?],
                        transform: [floats(&mut input)?, floats(&mut input)?],
                    };
                    if uint(&mut input)? == 0 {
                        frame.interact(&mut renderer, shape, appearance, fill, None)?;
                    } else {
                        let mut values = [0.0; 8];
                        for value in &mut values {
                            *value = f64::from_le_bytes(bytes(&mut input)?);
                        }
                        let [opacity, big_opacity, scale, diameter, x, y, ox, oy] = values;
                        let glow = Glow { opacity, big_opacity, scale, diameter, position: [x,y] };
                        frame.interact(&mut renderer, shape, appearance, fill,
                            Some(LightSpot { glow: &glow, origin: [ox,oy] }))?;
                    }
                }
                _ => return Err("unsupported frame stage".into()),
            }
        }
        for mut pixel in frame.into_pixels() {
            let alpha = pixel[3];
            if straight_output {
                for channel in &mut pixel[..3] {
                    *channel = if alpha == 0.0 { 0.0 } else { *channel / alpha };
                }
            }
            for v in pixel {
                output.write_all(&v.to_le_bytes())?;
            }
        }
        output.flush()?;
    }
    eprintln!("{} {:?}", renderer.device()?, renderer.stats());
    Ok(())
}
