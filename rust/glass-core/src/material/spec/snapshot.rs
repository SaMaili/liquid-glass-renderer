use super::*;

fn f32_at(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset+4].try_into().unwrap())
}

fn f64_at(data: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes(data[offset..offset+8].try_into().unwrap())
}

fn tag(data: &[u8], offset: usize) -> Result<bool> {
    match data[offset] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!("invalid Spec tag at byte {offset}")),
    }
}

fn scale<T: ScaleValue>(data: &[u8], offset: usize, width: usize, read: fn(&[u8], usize) -> T) -> Result<ParameterScale<T>> {
    let variant = data[offset + if width==4 { 41 } else { 57 }];
    let value = |relative| read(data, offset+relative);
    let optional = |relative| -> Result<Option<T>> {
        Ok(if tag(data, offset+relative+width)? { None } else { Some(value(relative)) })
    };
    match variant {
        0 => Ok(ParameterScale::Constant(value(0))),
        1 => Ok(ParameterScale::Linear {
            slope: value(0), intercept: value(width), lower: optional(width*2)?, upper: optional(width*4)?,
        }),
        2 => {
            let tail = 16+2*width;
            let transition = tail+2*width;
            let output = optional(tail)?;
            let absent = tag(data,offset+transition+8)?;
            let upper_tail = output.filter(|_| !absent)
                .map(|output| UpperTail { output, transition: f64_at(data,offset+transition) });
            Ok(ParameterScale::ClampedLinear {
                input: [f64_at(data,offset),f64_at(data,offset+8)],
                output: [value(16),value(16+width)], upper_tail,
            })
        }
        _ => Err("unsupported original material scale storage".into()),
    }
}

pub(super) fn decode(data: &[u8]) -> Result<MaterialSpec> {
    if data.len()!=0x700 {
        return Err("26A434 Spec requires 0x700 bytes".into());
    }
    let mut distances = [BlurDistance::Fixed(0.0); 4];
    for (index, distance) in distances.iter_mut().enumerate() {
        let offset = 0x144+index*8;
        let value = f32_at(data,offset);
        *distance = match data[offset+4] {
            0 => BlurDistance::Fixed(value),
            1 => BlurDistance::Inset(value),
            2 => BlurDistance::Outset(value),
            _ => return Err("invalid original blur distance tag".into()),
        };
    }
    let bounded = |offset| BoundedDistance {
        normalized: f64_at(data,offset),
        bounds: [f64_at(data,offset+8),f64_at(data,offset+16)],
    };
    Ok(MaterialSpec {
        blur: BlurSpec {
            opacity: f32_at(data,0x130),
            opacities: std::array::from_fn(|i| f32_at(data,0x134+i*4)),
            distances,
            radius: scale(data,0x168,4,f32_at)?,
            outer_refraction_blur_radius: if tag(data,0x198)? { None } else { Some(f32_at(data,0x194)) },
        },
        refraction: RefractionSpec {
            inner_height: bounded(0x1a0),
            inner_amount: bounded(0x1b8),
            outer_height: bounded(0x1d0),
            outer_amount: bounded(0x1e8),
            outer_opacity: f32_at(data,0x200),
        },
        edge_bleed: EdgeBleedSpec {
            opacity: scale(data,0x208,4,f32_at)?,
            normalized_height: f64_at(data,0x238),
            normalized_amount: f64_at(data,0x240),
            normalized_blur_radius: f64_at(data,0x248),
            max_blur_radius: f64_at(data,0x250),
            ycc: data[0x258..0x29d].try_into().unwrap(),
            use_darken_blending: tag(data,0x29d)?,
        },
        shadow: ShadowSpec {
            normalized_height: f64_at(data,0x10),
            normalized_amount: f64_at(data,0x18),
            blur_radius: f64_at(data,0x20),
            opacity: scale(data,0x28,4,f32_at)?,
            radius: scale(data,0x58,8,f64_at)?,
            offset: [f64_at(data,0x98),f64_at(data,0xa0)],
            ycc: data[0xa8..0xed].try_into().unwrap(),
            vibrancy_contribution: scale(data,0xf0,8,f64_at)?,
        },
        fill: BlurFill {
            radius: f32_at(data,0x48c),
            lighten: f32_at(data,0x490),
            darken: f32_at(data,0x494),
            normal: f32_at(data,0x498),
        },
    })
}
