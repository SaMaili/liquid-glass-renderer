//! Replay closed material Spec bytes for a direct comparison with original ARM64 blocks.
use liquid_glass_core::material::spec::{Dimensions, MaterialSpec};
use std::io::{self, BufReader, BufWriter, Read, Write};

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut output = [0; N];
    input.read_exact(&mut output)?;
    Ok(output)
}

fn option<const N: usize>(input: &mut impl Read) -> io::Result<Option<[u8; N]>> {
    match bytes::<1>(input)?[0] {
        0 => Ok(None),
        1 => Ok(Some(bytes(input)?)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData,"invalid option tag")),
    }
}

fn singles(output: &mut impl Write, values: impl IntoIterator<Item=f32>) -> io::Result<()> {
    for value in values { output.write_all(&value.to_le_bytes())?; }
    Ok(())
}

fn doubles(output: &mut impl Write, values: impl IntoIterator<Item=f64>) -> io::Result<()> {
    for value in values { output.write_all(&value.to_le_bytes())?; }
    Ok(())
}

fn main() -> Result<(),Box<dyn std::error::Error>> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    loop {
        let mode = bytes::<1>(&mut input)?[0];
        if mode==0 { break; }
        if mode>3 { return Err("invalid fixture mode".into()); }
        let dimensions = Dimensions {
            minimum: option(&mut input)?.map(f64::from_le_bytes),
            largest_area_minimum: option(&mut input)?.map(f64::from_le_bytes),
        };
        let layers = u64::from_le_bytes(bytes(&mut input)?);
        let pixel_length = f64::from_le_bytes(bytes(&mut input)?);
        let applicable = bytes::<1>(&mut input)?[0]!=0;
        let mut spec = MaterialSpec::from_26a434_snapshot(&bytes::<0x700>(&mut input)?)?;
        if mode==2 {
            let amount = option(&mut input)?.map(f32::from_le_bytes);
            spec.blur.apply_clear_diffusion(amount, applicable, &mut spec.fill)?;
        }
        let mut resolved = spec.resolve(dimensions,layers,pixel_length,applicable)?;
        if mode==3 { resolved.canonicalize_inactive_radii(); }
        let b = resolved.blur;
        singles(&mut output,[b.radius].into_iter().chain(b.distances).chain(b.opacities).chain([b.opacity]))?;
        let r = resolved.refraction;
        doubles(&mut output,[r.inner_height,r.inner_amount,r.outer_height,r.outer_amount,
            r.outer_distances[0],r.outer_distances[1]])?;
        singles(&mut output,[r.outer_opacity])?;
        let e = resolved.edge_bleed;
        doubles(&mut output,[e.amount,e.height,e.blur_radius])?;
        singles(&mut output,[e.opacity,e.distances[0],e.distances[1]])?;
        output.write_all(&spec.edge_bleed.ycc)?;
        output.write_all(&[u8::from(spec.edge_bleed.use_darken_blending)])?;
        let s = resolved.shadow;
        doubles(&mut output,spec.shadow.offset.into_iter().chain([s.amount,s.height,s.inset,s.blur_radius,s.radius]))?;
        output.write_all(&spec.shadow.ycc)?;
        singles(&mut output,[s.opacity])?;
        doubles(&mut output,[s.vibrancy_contribution])?;
        singles(&mut output,[spec.fill.radius,spec.fill.lighten,spec.fill.darken,spec.fill.normal])?;
        output.write_all(&[u8::from(resolved.fill.is_some())])?;
    }
    output.flush()?;
    Ok(())
}
