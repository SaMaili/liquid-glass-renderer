//! Binary fixture bridge for work/rust-scene/verify.py.
use liquid_glass_core::{
    Renderer,
    raster::{Content, Scale, Viewport},
    scene::{self, Backdrop},
};
use std::io::{self, BufReader, BufWriter, Read, Write};

mod fixture;
use fixture::uint;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    let viewport = Viewport::new(uint(&mut input)?, uint(&mut input)?, Scale::One)?;
    let mut renderer = Renderer::new(viewport.pixels())?;
    while let count @ 1..=64 = uint(&mut input)? {
        let backdrop = match uint(&mut input)? {
            0 => Backdrop::Original,
            1 => Backdrop::Current,
            _ => return Err("unsupported backdrop mode".into()),
        };
        let mut raster = vec![0; viewport.pixels().pixels() * 16];
        input.read_exact(&mut raster)?;
        let pixels = raster.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
        let content = Content::new(viewport, 0, pixels)?;
        let fixtures = fixture::groups(&mut input, count)?;
        let groups = fixture::borrow_groups(&fixtures);
        let image = scene::render(&mut renderer, &content, backdrop, &groups)?;
        for value in image {
            output.write_all(&value.to_le_bytes())?;
        }
        output.flush()?;
    }
    eprintln!("{} {:?}", renderer.device()?, renderer.stats());
    Ok(())
}
