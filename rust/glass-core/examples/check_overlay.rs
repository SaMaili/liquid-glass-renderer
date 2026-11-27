//! Verify the Rust overlays against externally supplied frozen v8 fixtures.
use liquid_glass_core::{
    Size,
    interaction::Glow,
    overlay::{Appearance, Fill, Scale, composite_fill, composite_glow},
};
use std::{fs::File, io::{BufReader, Read}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("supply a generated overlay fixture")?;
    let mut input = BufReader::new(File::open(path)?);
    let mut counts = [0; 2];
    let mut max_error = [0.0f32; 2];
    let mut quantized_delta = [0i32; 2];
    loop {
        let mut header = [0; 84];
        if input.read(&mut header[..1])? == 0 {
            break;
        }
        input.read_exact(&mut header[1..])?;
        let uint = |i: usize| u32::from_le_bytes(header[i*4..i*4+4].try_into().unwrap());
        let double = |i: usize| f64::from_le_bytes(header[20+i*8..28+i*8].try_into().unwrap());
        let size = Size::new(uint(0), uint(1))?;
        let pixels = size.width as usize * size.height as usize;
        let appearance = if uint(3) == 0 { Appearance::Light } else { Appearance::Dark };
        let scale = match uint(4) {
            1 => Scale::One,
            2 => Scale::Two,
            _ => return Err("invalid fixture scale".into()),
        };
        let read_floats = |input: &mut BufReader<File>, count: usize| -> std::io::Result<Vec<f32>> {
            let mut raw = vec![0; count * 4];
            input.read_exact(&mut raw)?;
            Ok(raw.chunks_exact(4).map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect())
        };
        let mut image: Vec<[f32; 4]> = read_floats(&mut input, pixels * 4)?
            .chunks_exact(4).map(|p| p.try_into().unwrap()).collect();
        let coverage = read_floats(&mut input, pixels)?;
        let expected = read_floats(&mut input, pixels * 4)?;
        match uint(2) {
            3 => composite_glow(size, &mut image, &coverage, appearance, &Glow {
                opacity: double(0), big_opacity: double(1), scale: double(2),
                diameter: double(3), position: [double(4), double(5)],
            }, [double(6), double(7)], scale)?,
            value => {
                let fill = match value {
                    0 => Fill::Idle,
                    1 => Fill::Rollover,
                    2 => Fill::Pressed,
                    _ => return Err("invalid fill fixture".into()),
                };
                composite_fill(size, &mut image, &coverage, appearance, fill)?;
            }
        }
        let index = scale.factor() as usize - 1;
        for (a, b) in image.into_iter().flatten().zip(expected) {
            let error = (a-b).abs();
            assert!(error <= 0.000002, "overlay case {:?}, op {}, actual {a}, expected {b}", counts, uint(2));
            max_error[index] = max_error[index].max(error);
            let quantize = |v: f32| (v.clamp(0.0, 1.0)*255.0).round_ties_even() as i32;
            let delta = (quantize(a)-quantize(b)).abs();
            assert!(delta <= 1);
            quantized_delta[index] = quantized_delta[index].max(delta);
        }
        counts[index] += 1;
    }
    assert!(counts.iter().all(|n| *n > 0));
    println!("cases {counts:?}, max absolute error {max_error:?}, max 8-bit channel delta {quantized_delta:?}");
    Ok(())
}
