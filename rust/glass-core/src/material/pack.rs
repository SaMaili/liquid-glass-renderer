use super::*;

// Original LPF context layout, shaders/uniform-layout.json.
fn put(buffer: &mut [u8], offset: usize, values: &[f32]) {
    for (out, value) in buffer[offset..offset + values.len() * 4].chunks_exact_mut(4).zip(values) {
        out.copy_from_slice(&value.to_ne_bytes());
    }
}

fn all_finite(values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err("finite material inputs required".into())
    }
}

fn reciprocal(value: f64) -> f32 {
    (1.0 / value) as f32
}

pub(super) fn binary16_bits(value: f32) -> Result<u16> {
    if !value.is_finite() || value.abs() >= 65520.0 {
        return Err("value outside binary16 range".into());
    }
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let significand = (bits & 0x7fffff) | 0x800000;
    if exponent < -25 {
        return Ok(sign);
    }
    // Subnormal inputs move the implicit bit into the low ten bits.
    let shift = 13 + (-14 - exponent).max(0);
    let retained = significand >> shift;
    let round = (1u32 << (shift - 1)) - 1 + (retained & 1);
    let rounded = (significand + round) >> shift;
    let encoded = ((exponent + 14).max(0) as u32) * 1024 + rounded;
    Ok(sign | encoded as u16)
}

fn matrix(buffer: &mut [u8], offset: usize, rows: &[[f32; 5]; 4]) -> Result<()> {
    if !rows.iter().flatten().all(|v| v.is_finite()) {
        return Err("finite color matrix required".into());
    }
    for (row, values) in rows[..3].iter().enumerate() {
        for (column, source) in [0, 1, 2, 4].into_iter().enumerate() {
            let word = binary16_bits(values[source])?;
            let start = offset + row * 8 + column * 2;
            buffer[start..start + 2].copy_from_slice(&word.to_ne_bytes());
        }
    }
    Ok(())
}

fn geometry(
    geometry: &Geometry, shadow: [f64; 2], vertical: f64,
) -> Result<([f32; 4], [f32; 2], [f32; 2])> {
    let [a,b,c,d] = geometry.transform;
    let [sx,sy] = shadow;
    if geometry.source_size.into_iter().chain(geometry.target_size)
        .any(|v| v == 0 || v > i32::MAX as u32)
        || !geometry.transform.into_iter().chain(geometry.sdf_transform.into_iter().flatten())
            .chain([geometry.source_scale,geometry.output_scale]).all(f32::is_finite)
    {
        return Err("finite transforms and positive surface sizes required".into());
    }
    let [w,h] = geometry.source_size.map(|v| v as f32);
    let scale = geometry.source_scale;
    let displacement = [
        0.0f32.mul_add(b, a * scale / w),
        0.0f32.mul_add(d, c * scale / w),
        0.0f32.mul_add(a, b * scale / h),
        0.0f32.mul_add(c, d * scale / h),
    ];
    let (shadow_offset,ring) = match geometry.sdf_transform {
        None => {
            let [w,h] = geometry.target_size.map(|v| v as f32);
            let scale = -geometry.output_scale;
            let x = f64::from(c).mul_add(sy,f64::from(a)*sx) as f32;
            let y = f64::from(b).mul_add(sx,f64::from(d)*sy) as f32;
            let vertical = scale * vertical as f32;
            ([x*scale/w,y*scale/h], [c*vertical/w,d*vertical/h])
        }
        Some(target) if target == geometry.transform =>
            ([-(sx as f32),-(sy as f32)], [0.0,-(vertical as f32)]),
        Some([aa,bb,cc,dd]) => {
            let determinant = dd * aa - cc * bb;
            if determinant == 0.0 {
                return Err("singular unequal SDF transform".into());
            }
            let inverse = (1.0 / f64::from(determinant)) as f32;
            let [ci,bi,di,ai] = [cc,bb,dd,aa].map(|v| v * inverse);
            let relative = [
                a.mul_add(di,-(ci*b)), b.mul_add(ai,-(bi*a)),
                c.mul_add(di,-(ci*d)), d.mul_add(ai,-(bi*c)),
            ];
            let x = relative[2].mul_add(-(sy as f32),-(relative[0] * sx as f32));
            let y = relative[3].mul_add(-(sy as f32),-(relative[1] * sx as f32));
            let vertical = -(vertical as f32);
            ([x,y], [(-d).mul_add(ci,c*di)*vertical,
                     (-c).mul_add(bi,d*ai)*vertical])
        }
    };
    Ok((displacement,shadow_offset,ring))
}

fn parameters(buffer: &mut [u8], extension: &mut [u8], p: &BackgroundParameters) -> Result<()> {
    let BackgroundParameters { blur, refraction:r, edge_bleed:e, shadow:s, fill, render_scale:scale, .. } = p;
    all_finite(&[r.inner_height,r.inner_amount,r.outer_height,r.outer_amount,
        r.outer_distances[0],r.outer_distances[1],e.amount,e.height,e.blur_radius,
        s.amount,s.height,s.blur_radius,s.radius,s.inset,s.vibrancy_contribution])?;
    if ![blur.radius,blur.opacity,r.outer_opacity,e.opacity,s.opacity,*scale]
        .into_iter().chain(blur.opacities).chain(blur.distances).chain(e.distances).all(f32::is_finite) || *scale<=0.0
    {
        return Err("finite material scalars and a positive render scale required".into());
    }
    let [a,b,c,d] = blur.opacities.map(|value| value * blur.opacity);
    put(buffer,160,&[a,a-b,b-c,c-d]);
    put(buffer,176,&blur.distances);
    let radius = if e.opacity > 0.0 { (e.blur_radius * 0.5) as f32 } else { 0.0 };
    put(buffer,16,&[r.inner_amount as f32,reciprocal(r.inner_height),r.outer_amount as f32,
        reciprocal(r.outer_height),r.outer_distances[0] as f32,r.outer_distances[1] as f32,
        (blur.radius * 0.5) * scale,radius * scale,e.amount as f32,reciprocal(e.height),
        s.amount as f32,reciprocal(s.height)]);
    let contribution = if s.opacity > 0.0 { s.vibrancy_contribution } else { 0.0 };
    let radius = if contribution > 0.0 { (s.blur_radius * 0.5) as f32 } else { 0.0 };
    put(buffer,72,&[radius*scale,reciprocal(s.radius)]);
    put(buffer,152,&[contribution as f32,p.shadow_face_opacity]);
    put(buffer,192,&[e.distances[0],e.distances[1],e.opacity.clamp(0.0,1.0)]);
    put(buffer,216,&[s.inset as f32,s.opacity.clamp(0.0,1.0),r.outer_opacity]);
    if let Some(fill) = fill {
        if ![fill.radius,fill.lighten,fill.darken,fill.normal].into_iter().all(f32::is_finite) {
            return Err("finite BlurFill inputs required".into());
        }
        let [mut light,mut dark,normal] = [fill.lighten,fill.darken,fill.normal].map(|v| v.clamp(0.0,1.0));
        let active = light > 0.0 || dark > 0.0 || normal > 0.0;
        let sum = light + dark;
        if sum > 1.0 {
            light /= sum;
            dark /= sum;
        }
        let radius = if active { (fill.radius*0.5) * scale } else { 0.0 };
        put(buffer,292,&[radius,light,dark,normal]);
        extension[21] = u8::from(active);
    }
    Ok(())
}

fn effects(buffer: &mut [u8], extension: &mut [u8], p: &BackgroundParameters) -> Result<()> {
    let e = &p.effects;
    let h = &e.highlights;
    all_finite(&[e.face_opacity,e.shadow_offset[0],e.shadow_offset[1],e.ring_vertical_offset,
        e.ring_stroke_width,e.ring_radius,e.ring_opacity,e.ring_mask,
        e.sdr_shadow_distances[0],e.sdr_shadow_distances[1],e.aberration_amount,e.aberration_height,
        e.aberration_offset,e.aberration_angle,e.sdr_white_value,e.clamp_limit,
        h.height,h.angle,h.spread,h.spread_sdr,h.amount,h.effect_offset,h.color_bias])?;
    if ![e.context_gamma,e.headroom_fraction,e.clamp_override].into_iter().all(f32::is_finite) || e.context_gamma<=0.0 {
        return Err("finite effect context with positive gamma required".into());
    }
    let (displacement,shadow,ring) = geometry(&p.geometry,e.shadow_offset,e.ring_vertical_offset)?;
    put(buffer,0,&displacement);
    put(buffer,64,&shadow);
    put(buffer,240,&ring);
    let bleed = if e.bleed_blend_mode == 0 { [-1.0,1.0] } else { [1.0,0.0] };
    put(buffer,204,&[e.face_opacity.clamp(0.0,1.0) as f32,bleed[0],bleed[1]]);
    let holding = if e.holding_tone == 0 { 0.0 } else { 1.0-e.headroom_fraction };
    let distance = e.sdr_shadow_distances[0] as f32;
    let delta = (e.sdr_shadow_distances[1]-f64::from(distance)) as f32;
    // Original constants at 0x18b3ef910 and 0x18b3ef9e4.
    let inverse = if delta.abs() < 0.0001 { 10000.0 } else { reciprocal(f64::from(delta)) };
    put(buffer,228,&[holding,distance,inverse]);
    put(buffer,248,&[e.ring_stroke_width as f32,e.ring_radius as f32,
        e.ring_opacity.clamp(0.0,1.0) as f32,e.ring_mask as f32]);
    let spread = h.spread as f32;
    let spread_sdr = h.spread_sdr as f32;
    let angle = (spread - spread_sdr).mul_add(e.headroom_fraction,spread_sdr);
    let amount = (1.0/h.amount-2.0) as f32;
    put(buffer,264,&[h.angle.sin() as f32,-(h.angle.cos() as f32), h.height as f32,
        angle.cos(),amount,h.effect_offset as f32,h.color_bias as f32]);
    let angle = e.aberration_angle as f32;
    put(buffer,308,&[e.aberration_amount as f32,angle.cos(),angle.sin()]);
    let limit = if e.clamp_override == 0.0 { e.clamp_limit as f32 } else { e.clamp_override };
    let clamp = limit.powf(1.0 / e.context_gamma);
    put(extension,0,&[reciprocal(e.aberration_height),e.aberration_offset as f32,
        e.sdr_white_value as f32,clamp,p.face_color_matrix_max_luma_complement]);
    let offset = h.effect_offset as f32;
    extension[20] = u8::from(e.force_preserve_hue || e.preserve_hue!=0);
    extension[22] = u8::from(offset < 0.0 && f64::from((h.height as f32+offset).abs())<0.001);
    extension[23] = u8::from(offset < 0.0);
    extension[24] = u8::from(h.color_bias as f32!=0.0);
    Ok(())
}

pub(super) fn background(p: &BackgroundParameters, style: BackgroundStyle) -> Result<BackgroundMaterial> {
    let mut uniforms = [0; 320];
    let mut extension = [0; 28];
    parameters(&mut uniforms,&mut extension,p)?;
    effects(&mut uniforms,&mut extension,p)?;
    matrix(&mut uniforms,80,&p.colors.face)?;
    matrix(&mut uniforms,104,&p.colors.bleed)?;
    matrix(&mut uniforms,128,&p.colors.shadow)?;
    if !style.refraction_gain.is_finite() || style.key_fill_amount.is_some_and(|v| !v.is_finite()) {
        return Err("finite background presentation controls required".into());
    }
    for (offset, amount) in [(16, p.refraction.inner_amount), (24, p.refraction.outer_amount)] {
        // The control multiplies the already rounded CA shader displacement.
        let amount = (f64::from(amount as f32) * style.refraction_gain) as f32;
        put(&mut uniforms, offset, &[amount]);
    }
    if let Some(amount) = style.key_fill_amount {
        put(&mut uniforms, 280, &[amount]);
    }
    for (i, bytes) in uniforms.chunks_exact(4).enumerate() {
        let offset = i*4;
        if (80..152).contains(&offset) {
            continue;
        }
        let value = f32::from_ne_bytes(bytes.try_into().unwrap());
        let allow_inf = [20,28,52,60,76,280].contains(&offset);
        if value.is_nan() || (value.is_infinite() && !allow_inf) {
            return Err(format!("material uniform outside Float32 range at byte {offset}"));
        }
    }
    for (i, bytes) in extension[..20].chunks_exact(4).enumerate() {
        let value = f32::from_ne_bytes(bytes.try_into().unwrap());
        if value.is_nan() || (i!=0 && value.is_infinite()) {
            return Err(format!("material extension outside Float32 range at byte {}",i*4));
        }
    }
    Ok(BackgroundMaterial { uniforms,extension })
}
