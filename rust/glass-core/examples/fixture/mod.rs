use liquid_glass_core::scene::{Group, Material, MaterialKind, Shape};
use std::io::{self, Read};

pub fn bytes<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut data = [0; N];
    reader.read_exact(&mut data)?;
    Ok(data)
}

pub fn uint(reader: &mut impl Read) -> io::Result<u32> {
    Ok(u32::from_le_bytes(bytes(reader)?))
}

pub fn floats<const N: usize>(reader: &mut impl Read) -> io::Result<[f32; N]> {
    let mut out = [0.0; N];
    for value in &mut out {
        *value = f32::from_le_bytes(bytes(reader)?);
    }
    Ok(out)
}

pub struct FixtureGroup {
    shapes: Vec<Shape<'static>>,
    uniforms: [u8; 320],
    extension: [u8; 32],
    smoothness: f32,
    opacity: f32,
    blur_levels: u32,
}

pub fn groups(reader: &mut impl Read, count: u32) -> Result<Vec<FixtureGroup>, Box<dyn std::error::Error>> {
    let mut fixtures = Vec::new();
    for _ in 0..count {
        let shape_count = uint(reader)?;
        if !(1..=64).contains(&shape_count) {
            return Err("unsupported shape count".into());
        }
        let [smoothness, opacity] = floats(reader)?;
        let blur_levels = uint(reader)?;
        let uniforms = bytes(reader)?;
        let extension = bytes(reader)?;
        let mut shapes = Vec::new();
        for _ in 0..shape_count {
            shapes.push(Shape::Geometry {
                values: [floats(reader)?, floats(reader)?, floats(reader)?],
                transform: [floats(reader)?, floats(reader)?],
            });
        }
        fixtures.push(FixtureGroup { shapes, uniforms, extension, smoothness, opacity, blur_levels });
    }
    Ok(fixtures)
}

pub fn borrow_groups(fixtures: &[FixtureGroup]) -> Vec<Group<'_>> {
    fixtures.iter().map(|g| Group {
            shapes: &g.shapes,
            shape_tags: &[],
            tints: &[],
            material: Material { kind: MaterialKind::All, uniforms: &g.uniforms, extension: &g.extension },
            opacity: g.opacity,
            blur_levels: g.blur_levels,
            smoothness: g.smoothness,
        }).collect()
}
