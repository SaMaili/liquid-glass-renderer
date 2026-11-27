//! Caller-ordered glass groups using the recovered GLSL and linear SDR composition.
use std::{borrow::Cow, rc::Rc, sync::LazyLock};

use crate::{MipFilter, Pass, Raster, Renderer, Result, Sampler, Size, raster::{Content, Patch, PixelRect}};

const SDF_PROBE: &str = include_str!("../../../shaders/sdf-probe.frag");
const SDF_UNION: &str = include_str!("../../../shaders/sdf-union.frag");
static SDF_UNION_WITH_TIES: LazyLock<String> = LazyLock::new(|| {
    SDF_UNION.replacen("#version 450\n", "#version 450\n#define RESOLVE_UNION_TIES\n", 1)
});
const SDF_TINT: &str = include_str!("../../../shaders/sdf-tint.frag");
const BLUR: &str = include_str!("../../../shaders/blur-agx2.frag");
const GEOMETRY_VERTEX: &str = r#"#version 450
layout(location=0) out vec2 shape;
layout(location=1) out vec2 source;
layout(std140,binding=10) uniform ShapeTransform { vec4 row0; vec4 row1; } transform;
void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);
shape=vec2(dot(vec3(p,1),transform.row0.xyz),dot(vec3(p,1),transform.row1.xyz));
source=p;gl_Position=vec4(p*2-1,0,1);}
"#;

#[derive(Clone, Copy)]
pub enum MaterialKind {
    Foreground,
    Minimal,
    Edge,
    Refraction,
    RefractionEdge,
    Composite,
    CompositeEdge,
    CompositeRefraction,
    All,
}

impl MaterialKind {
    fn shader(self) -> &'static str {
        match self {
            Self::Foreground => include_str!("../../../shaders/foreground.frag"),
            Self::Minimal => include_str!("../../../shaders/background-minimal.frag"),
            Self::Edge => include_str!("../../../shaders/background-edge.frag"),
            Self::Refraction => include_str!("../../../shaders/background-refraction.frag"),
            Self::RefractionEdge => include_str!("../../../shaders/background-refraction-edge.frag"),
            Self::Composite => include_str!("../../../shaders/background-composite.frag"),
            Self::CompositeEdge => include_str!("../../../shaders/background-composite-edge.frag"),
            Self::CompositeRefraction => include_str!("../../../shaders/background-composite-refraction.frag"),
            Self::All => include_str!("../../../shaders/background-all.frag"),
        }
    }
}

pub struct Material<'a> {
    pub kind: MaterialKind,
    pub uniforms: &'a [u8],
    pub extension: &'a [u8],
}

impl Material<'_> {
    fn validate(&self) -> Result<()> {
        let valid = if matches!(self.kind, MaterialKind::Foreground) {
            self.uniforms.len() == 80 && self.extension.is_empty()
        } else {
            self.uniforms.len() == 320 && matches!(self.extension.len(), 28 | 32)
        };
        if !valid {
            return Err("material payload does not match its shader".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum Shape<'a> {
    Field(Raster<'a>),
    /// Recovered three-vec4 geometry payload and scene-UV-to-shape transform.
    Geometry { values: [[f32; 4]; 3], transform: [[f32; 3]; 2] },
}

/// Select shapes by comparing their masked tag bits to `value`.
#[derive(Clone, Copy)]
pub struct ShapeFilter {
    pub mask: u64,
    pub value: u64,
}

impl ShapeFilter {
    pub const ALL: Self = Self { mask: 0, value: 0 };

    pub const fn exact(tags: u64) -> Self {
        Self { mask: u64::MAX, value: tags }
    }

    fn matches(self, tags: u64) -> bool {
        tags & self.mask == self.value
    }
}

/// Signed distances in logical points. Negative values lie inside the shape.
#[derive(Clone, Copy)]
pub enum TintRamp {
    Sharp { edge: f32 },
    Soft { opaque_through: f32, transparent_at: f32 },
}

impl TintRamp {
    /// The 26A434 recipe uses opaque stops at -1 and 0, then fades to 10 points.
    pub const SOFT: Self = Self::Soft { opaque_through: 0.0, transparent_at: 10.0 };
    /// The sharp recipe moves the transparent stop from 10 to 0 points.
    pub const SHARP: Self = Self::Sharp { edge: 0.0 };

    pub(crate) fn parameters(self) -> [f32; 3] {
        match self {
            Self::Sharp { edge } => [edge, edge, 1.0],
            Self::Soft { opaque_through, transparent_at } => [opaque_through, transparent_at, 0.0],
        }
    }
}

/// A backdrop-aware color-matrix layer over the rendered glass, in linear sRGB.
#[derive(Clone, Copy)]
pub struct Tint {
    pub shape_filter: ShapeFilter,
    pub ramp: TintRamp,
    pub opacity: f32,
    /// Row-major 4×5 matrix applied to straight RGB, alpha, and a constant 1.
    pub matrix: [[f32; 5]; 4],
    /// A positive value clamps the matrix output. Zero disables clamping.
    pub max_color_component: f32,
    pub preserves_hue: bool,
}

impl Tint {
    fn validate(self) -> Result<()> {
        let [start, end, sharp] = self.ramp.parameters();
        if self.shape_filter.value & !self.shape_filter.mask != 0
            || !(0.0..=1.0).contains(&self.opacity)
            || !(0.0..=f32::MAX).contains(&self.max_color_component)
            || !self.matrix.iter().flatten().all(|v| v.is_finite())
            || !start.is_finite() || !end.is_finite()
            || (sharp == 0.0 && (end <= start || !(end - start).is_finite()))
        {
            return Err("invalid glass tint".into());
        }
        Ok(())
    }
}

pub struct Group<'a> {
    pub shapes: &'a [Shape<'a>],
    /// Tags in shape order. An empty slice gives every shape tag zero.
    pub shape_tags: &'a [u64],
    pub material: Material<'a>,
    /// Applied in slice order after the material pass.
    pub tints: &'a [Tint],
    pub opacity: f32,
    pub smoothness: f32,
    /// AGX2 levels for the viewport's pixel raster, counting the original image.
    /// The caller plans them from the blur radius in raster pixels.
    pub blur_levels: u32,
}

#[derive(Clone, Copy)]
pub enum Backdrop {
    Original,
    Current,
}

fn input<'a>(size: Size, rgba: &'a [f32], mipmaps: &'a [Raster<'a>], mip: MipFilter) -> crate::TextureInput<'a> {
    crate::TextureInput {
        image: Raster { size, rgba },
        mipmaps,
        sampler: Sampler { mip, ..Sampler::default() },
    }
}

fn words<const N: usize, const B: usize>(values: [f32; N]) -> [u8; B] {
    assert_eq!(N * 4, B);
    let mut out = [0; B];
    for (value, bytes) in values.into_iter().zip(out.chunks_exact_mut(4)) {
        bytes.copy_from_slice(&value.to_ne_bytes());
    }
    out
}

fn mip_capacity(size: Size) -> u32 {
    u32::BITS - size.width.max(size.height).leading_zeros()
}

/// Select the full-image AGX2 pyramid size from the recovered s1 scalar.
/// Uses the same f32 log2 decision as the portable Python planner.
pub fn blur_level_count(size: Size, s1: f32) -> Result<u32> {
    size.validate()?;
    if !s1.is_finite() || s1 < 0.0 {
        return Err("s1 must be finite and nonnegative".into());
    }
    // The reference rounds only after multiplication by the original coefficient.
    let scaled = (f64::from(s1) * 1.6) as f32;
    if !scaled.is_finite() {
        return Err("scaled blur radius overflow".into());
    }
    let requested = if scaled == 0.0 {
        1
    } else {
        (scaled.log2().ceil().max(0.0) as u32 + 1).max(2)
    };
    Ok(requested.min(mip_capacity(size)))
}

const DUMMY: Size = Size { width: 1, height: 1 };
const ZERO: [f32; 4] = [0.0; 4];

pub(crate) fn field<'a>(renderer: &mut Renderer, shape: Shape<'a>) -> Result<Cow<'a, [f32]>> {
    match shape {
        Shape::Field(raster) => {
            raster.validate()?;
            if raster.size != renderer.size() {
                return Err("distance field does not match the viewport".into());
            }
            Ok(Cow::Borrowed(raster.rgba))
        }
        Shape::Geometry { values, transform } => {
            if !values.iter().flatten().chain(transform.iter().flatten()).all(|v| v.is_finite()) {
                return Err("finite geometry and transform required".into());
            }
            let mode = values[0][2];
            if mode.abs() > 2_147_483_000.0 || mode.fract() != 0.0 {
                return Err("integer geometry mode required".into());
            }
            if values[0][0] <= 0.0 || values[0][1] <= 0.0 {
                return Err("positive shape extents required".into());
            }
            let invalid_radius = if mode.abs() <= 4.0 {
                values[2][2] == 0.0
            } else {
                values[2].contains(&0.0)
            };
            if invalid_radius {
                return Err("zero geometry radius is outside the validated shader domain".into());
            }
            let [a, b] = transform;
            let transform = words::<8, 32>([a[0], a[1], a[2], 0.0, b[0], b[1], b[2], 0.0]);
            let uniforms = words::<12, 48>(std::array::from_fn(|i| values[i / 4][i % 4]));
            let output = renderer.render(&Pass {
                fragment: SDF_PROBE, vertex: Some(GEOMETRY_VERTEX),
                source: input(DUMMY, &ZERO, &[], MipFilter::None),
                sdf: input(DUMMY, &ZERO, &[], MipFilter::None),
                noise: None, uniforms: &uniforms, extension: &[], edr: 1.0,
                extra_uniforms: &[(10, &transform)], vertex_count: 3,
            })?;
            Ok(Cow::Owned(output))
        }
    }
}

fn union(renderer: &mut Renderer, a: &[f32], b: &[f32], smoothness: f32) -> Result<Vec<f32>> {
    let mut ties = false;
    for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let (distance, dot) = if b[3] == 0.0 { (10_000.0, 0.0) } else {
            (b[0], a[1] * b[1] + a[2] * b[2])
        };
        let k = (0.5 - 0.5 * dot).clamp(0.0, 1.0) * smoothness;
        let delta = distance - a[0];
        if !k.is_finite() || !delta.is_finite() {
            return Err("overflowing union arithmetic".into());
        }
        ties |= k == 0.0 && delta == 0.0;
    }
    let size = renderer.size();
    renderer.render(&Pass {
        fragment: if ties { &SDF_UNION_WITH_TIES } else { SDF_UNION }, vertex: None,
        source: input(size, a, &[], MipFilter::None),
        sdf: input(size, b, &[], MipFilter::None),
        noise: None, uniforms: &words::<4,16>([smoothness, 0.0, 0.0, 0.0]),
        extension: &[], edr: 1.0, extra_uniforms: &[], vertex_count: 3,
    })
}

struct BlurLevel {
    size: Size,
    rgba: Vec<f32>,
}

fn blur(renderer: &mut Renderer, base: Raster<'_>, levels: u32) -> Result<Vec<BlurLevel>> {
    let mut output: Vec<BlurLevel> = Vec::new();
    for _ in 1..levels {
        let prior = output.last().map_or(base, |v| Raster { size: v.size, rgba: &v.rgba });
        let size = Size { width: (prior.size.width / 2).max(1), height: (prior.size.height / 2).max(1) };
        let uniforms = words::<4,16>([1.0 / size.width as f32, 1.0 / size.height as f32, 0.0, 0.0]);
        let rgba = renderer.render_region(&Pass {
            fragment: BLUR, vertex: None,
            source: input(prior.size, prior.rgba, &[], MipFilter::Nearest),
            sdf: input(DUMMY, &ZERO, &[], MipFilter::Nearest),
            noise: None, uniforms: &uniforms, extension: &[], edr: 1.0,
            extra_uniforms: &[], vertex_count: 3,
        }, size)?;
        output.push(BlurLevel { size, rgba });
    }
    Ok(output)
}

fn validate_groups(size: Size, groups: &[Group<'_>]) -> Result<()> {
    let capacity = mip_capacity(size);
    for group in groups {
        group.material.validate()?;
        if group.shapes.is_empty() || !(0.0..=1.0).contains(&group.opacity)
            || !(0.0..=f32::MAX).contains(&group.smoothness)
            || !(1..=capacity).contains(&group.blur_levels)
            || (!group.shape_tags.is_empty() && group.shape_tags.len() != group.shapes.len())
        {
            return Err("invalid glass group".into());
        }
        for &tint in group.tints {
            tint.validate()?;
        }
    }
    Ok(())
}

type DistanceField<'a> = Rc<Cow<'a, [f32]>>;

fn merge_field<'a>(
    renderer: &mut Renderer,
    combined: &mut Option<DistanceField<'a>>,
    next: &DistanceField<'a>,
    smoothness: f32,
) -> Result<()> {
    match combined {
        Some(field) if field != next => *field = Rc::new(Cow::Owned(union(renderer, field, next, smoothness)?)),
        None => *combined = Some(Rc::clone(next)),
        _ => {}
    }
    Ok(())
}

struct GroupFields<'a> {
    union: DistanceField<'a>,
    retained: Vec<(u64, DistanceField<'a>)>,
}

fn group_fields<'a>(renderer: &mut Renderer, group: &Group<'a>) -> Result<GroupFields<'a>> {
    let mut combined = None;
    let mut retained = Vec::new();
    for (index, &shape) in group.shapes.iter().enumerate() {
        let next = Rc::new(field(renderer, shape)?);
        merge_field(renderer, &mut combined, &next, group.smoothness)?;
        let tags = group.shape_tags.get(index).copied().unwrap_or(0);
        if group.tints.iter().any(|tint| {
            tint.opacity != 0.0 && tint.shape_filter.mask != 0 && tint.shape_filter.matches(tags)
        }) {
            retained.push((tags, next));
        }
    }
    Ok(GroupFields { union: combined.unwrap(), retained })
}

impl<'a> GroupFields<'a> {
    fn for_tint(
        &self,
        renderer: &mut Renderer,
        group: &Group<'_>,
        filter: ShapeFilter,
    ) -> Result<Option<DistanceField<'a>>> {
        let matching = || self.retained.iter().filter(|(tags, _)| filter.matches(*tags));
        if filter.mask == 0 || matching().count() == group.shapes.len() {
            return Ok(Some(Rc::clone(&self.union)));
        }
        let mut combined = None;
        for (_, field) in matching() {
            merge_field(renderer, &mut combined, field, group.smoothness)?;
        }
        Ok(combined)
    }
}

fn apply_tints(
    renderer: &mut Renderer,
    group: &Group<'_>,
    fields: &GroupFields<'_>,
    destination: &[f32],
    layer: &[f32],
    damage: PixelRect,
) -> Result<Option<Vec<f32>>> {
    let size = renderer.size();
    let region = Size { width: damage.width, height: damage.height };
    let mut tinted = None;
    for tint in group.tints.iter().filter(|tint| tint.opacity != 0.0) {
        let Some(field) = fields.for_tint(renderer, group, tint.shape_filter)? else { continue };
        let pixels = tinted.get_or_insert_with(|| {
            let mut pixels = layer.to_vec();
            let back = damage.row_spans(size).flat_map(|span| destination[span].as_chunks::<4>().0);
            for (front, back) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(back) {
                let remaining = 1.0 - front[3];
                for (front, back) in front.iter_mut().zip(back) {
                    *front += back * remaining;
                }
            }
            pixels
        });
        let [start, end, sharp] = tint.ramp.parameters();
        let mut uniforms = [0.0; 28];
        // The shader consumes five RGBA columns followed by two vec4s.
        for (index, word) in uniforms[..20].iter_mut().enumerate() {
            *word = tint.matrix[index % 4][index / 4];
        }
        uniforms[20..].copy_from_slice(&[
            start, end, tint.opacity, tint.max_color_component,
            damage.x as f32, damage.y as f32, sharp, f32::from(u8::from(tint.preserves_hue)),
        ]);
        *pixels = renderer.render_rect(&Pass {
            fragment: SDF_TINT, vertex: None,
            source: input(region, pixels, &[], MipFilter::None),
            sdf: input(size, &field, &[], MipFilter::None),
            noise: Some(input(size, &fields.union, &[], MipFilter::None)),
            uniforms: &words::<28, 112>(uniforms), extension: &[], edr: 1.0,
            extra_uniforms: &[], vertex_count: 3,
        }, size, damage)?;
    }
    Ok(tinted)
}

/// Conservative radius in base raster pixels for sampling any generated mip.
/// Each AGX2 step has two output-texel offsets plus one input texel of
/// bilinear support. The final material samples with linear mip filtering.
fn mip_margin(dimension: u32, levels: u32) -> f64 {
    let mut previous = dimension;
    let mut reach = 0.0;
    let pitch = |width| f64::from(dimension) / f64::from(width);
    for _ in 1..levels {
        let next = (previous / 2).max(1);
        reach += 2.0 * pitch(next) + pitch(previous);
        previous = next;
    }
    reach + pitch(previous)
}

fn material_damage(group: &Group<'_>, sdf: &[f32], input: PixelRect, size: Size) -> Result<PixelRect> {
    let material = &group.material;
    let unbounded = std::cell::Cell::new(false);
    let word = |offset: usize| {
        let value = f32::from_ne_bytes(material.uniforms[offset..offset + 4].try_into().unwrap());
        if !value.is_finite() {
            unbounded.set(true);
        }
        f64::from(value).abs()
    };
    let foreground = matches!(material.kind, MaterialKind::Foreground);
    let minimal = matches!(material.kind, MaterialKind::Minimal);
    let refraction = matches!(material.kind, MaterialKind::Refraction | MaterialKind::RefractionEdge
        | MaterialKind::CompositeRefraction | MaterialKind::All);
    let edge = matches!(material.kind, MaterialKind::Edge | MaterialKind::CompositeEdge
        | MaterialKind::RefractionEdge | MaterialKind::All);
    let aberrates = matches!(material.kind, MaterialKind::RefractionEdge | MaterialKind::All);
    let composited_fill = matches!(material.kind, MaterialKind::Composite | MaterialKind::CompositeEdge
        | MaterialKind::CompositeRefraction | MaterialKind::All) && material.extension[21] != 0;

    let mut displacement = 0.0f64;
    let extra_taps;
    if foreground {
        displacement = word(16) * (word(64) + word(68))
            + word(28) * (word(40) + word(44));
        extra_taps = 0.0;
    } else {
        if refraction {
            displacement = word(16);
            if word(224) > 0.0 {
                displacement = displacement.max(word(24));
            }
        }
        if aberrates {
            // Aberration samples add to the inner refraction displacement.
            displacement = displacement.max(word(16) + word(308) * (word(312) + word(316)));
        }
        if edge {
            if word(152) > 0.000001 && word(220) > 0.0 {
                displacement = displacement.max(word(56));
            }
            displacement = displacement.max(word(48));
        }
        // FourTap and BlurFill shift by LOD/4 in base raster pixels.
        // Blur interpolation weights are individually clamped to [0, 1].
        let mut tap_radius = if composited_fill { word(292) } else { 0.0 };
        if aberrates && word(308) != 0.0 {
            let varying_radius = word(40) * [160,164,168,172].into_iter().map(word).sum::<f64>();
            tap_radius = tap_radius.max(varying_radius);
        }
        if tap_radius >= f64::from(f32::MAX) * 0.5 {
            unbounded.set(true);
        }
        extra_taps = tap_radius.ln_1p() / (4.0 * std::f64::consts::LN_2);
    }
    let uv = if minimal || displacement == 0.0 {
        0.0
    } else {
        let grad = sdf.chunks_exact(4).map(|p| f64::from(p[1].abs().max(p[2].abs())))
            .fold(0.0, f64::max);
        let row_norm = (word(0) + word(4)).max(word(8) + word(12));
        row_norm * grad * displacement
    };
    let radius = (uv * f64::from(size.width) + mip_margin(size.width,group.blur_levels))
        .max(uv * f64::from(size.height) + mip_margin(size.height,group.blur_levels))
        + extra_taps + 2.0;
    // Very large or unordered raw uniforms conservatively invalidate everything.
    let radius = if radius.is_finite() && !unbounded.get() { radius.ceil() as u32 } else { u32::MAX };
    input.expanded(radius,size)
}

struct Prior<'a> {
    stages: &'a [Vec<f32>],
    damage: PixelRect,
}

fn render_stages(
    renderer: &mut Renderer,
    content: &Content,
    backdrop: Backdrop,
    groups: &[Group<'_>],
    prior: Option<Prior<'_>>,
    mut snapshots: Option<&mut Vec<Vec<f32>>>,
) -> Result<(Vec<f32>, PixelRect)> {
    let size = renderer.size();
    let original = content.raster();
    if size != original.size {
        return Err("scene viewport does not match render target".into());
    }
    validate_groups(size,groups)?;
    let full = PixelRect::full(size);
    let input_damage = prior.as_ref().map_or(full, |prior| prior.damage);
    let mut damage = input_damage;
    let mut result = original.rgba.to_vec();
    let active = || groups.iter().filter(|g| g.opacity != 0.0);
    // Original-backdrop groups can share all levels generated for this frame.
    let cached = if matches!(backdrop, Backdrop::Original) {
        blur(renderer, original, active().map(|g| g.blur_levels).max().unwrap_or(1))?
    } else {
        Vec::new()
    };
    for (index,group) in active().enumerate() {
        let fields = group_fields(renderer, group)?;
        let sdf = &fields.union;
        if prior.is_some() && damage != full {
            let source_damage = match backdrop {
                Backdrop::Original => input_damage,
                Backdrop::Current => damage,
            };
            damage = damage.cover(material_damage(group,sdf,source_damage,size)?).align_quads(size);
        }

        let current;
        let (base, levels) = match backdrop {
            Backdrop::Original => (original, &cached[..group.blur_levels as usize - 1]),
            Backdrop::Current => {
                let base = Raster { size, rgba: &result };
                current = blur(renderer, base, group.blur_levels)?;
                (base, current.as_slice())
            }
        };
        let mips: Vec<_> = levels.iter().map(|v| Raster { size: v.size, rgba: &v.rgba }).collect();
        let layer = renderer.render_rect(&Pass {
            fragment: group.material.kind.shader(), vertex: None,
            source: input(size, base.rgba, &mips, MipFilter::Linear),
            sdf: input(size, sdf, &[], MipFilter::Nearest),
            noise: None, uniforms: group.material.uniforms, extension: group.material.extension,
            edr: 1.0, extra_uniforms: &[], vertex_count: 3,
        }, size, damage)?;

        let tinted = apply_tints(renderer, group, &fields, &result, &layer, damage)?;
        if let Some(previous) = &prior && damage != full {
            let mut next = previous.stages[index].clone();
            for span in damage.row_spans(size) {
                next[span.clone()].copy_from_slice(&result[span]);
            }
            result = next;
        }
        let tint_pixels = tinted.as_deref().map(|pixels| pixels.as_chunks::<4>().0);
        let mut index = 0;
        let rows = layer.as_chunks::<4>().0.chunks_exact(damage.width as usize);
        for (span, row) in damage.row_spans(size).zip(rows) {
            for (back, front) in result[span].as_chunks_mut::<4>().0.iter_mut().zip(row) {
                let factor = 1.0 - front[3] * group.opacity;
                let tint = tint_pixels.map(|pixels| pixels[index]);
                for (channel, back) in back.iter_mut().enumerate() {
                    let source = front[channel];
                    let plain = source * group.opacity + *back * factor;
                    // Apply group opacity once to the glass and its tint layers together.
                    *back = tint.map_or(plain, |tint| {
                        let full = source + *back * (1.0 - front[3]);
                        if tint[channel] == full { plain } else {
                            tint[channel] * group.opacity + *back * (1.0 - group.opacity)
                        }
                    });
                }
                index += 1;
            }
        }
        if let Some(snapshots) = snapshots.as_deref_mut() {
            snapshots.push(result.clone());
        }
    }
    if result.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite composited scene".into());
    }
    Ok((result,damage))
}

/// Render caller-planned groups in array order. All fields and the output
/// share the content viewport. The result is linear premultiplied sRGB.
pub fn render(
    renderer: &mut Renderer,
    content: &Content,
    backdrop: Backdrop,
    groups: &[Group<'_>],
) -> Result<Vec<f32>> {
    render_stages(renderer,content,backdrop,groups,None,None).map(|(pixels,_)| pixels)
}

/// Immutable scene geometry/material with versioned, mutable background pixels.
/// Stage outputs retain the unaffected pixels for accumulated-backdrop groups.
pub struct LiveScene<'a> {
    content: Content,
    backdrop: Backdrop,
    groups: &'a [Group<'a>],
    stages: Vec<Vec<f32>>,
}

impl<'a> LiveScene<'a> {
    /// The byte budget covers the resident input raster and stage snapshots.
    /// A transactional update can temporarily hold a second set of snapshots.
    pub fn new(
        renderer: &mut Renderer,
        content: Content,
        backdrop: Backdrop,
        groups: &'a [Group<'a>],
        resident_budget_bytes: usize,
    ) -> Result<Self> {
        let stage_count = 1 + groups.iter().filter(|g| g.opacity != 0.0).count();
        let raster_bytes = content.raster().rgba.len() * size_of::<f32>();
        if raster_bytes.checked_mul(stage_count).is_none_or(|n| n > resident_budget_bytes) {
            return Err("live scene exceeds stage raster budget".into());
        }
        let mut stages = Vec::new();
        render_stages(renderer,&content,backdrop,groups,None,Some(&mut stages))?;
        Ok(Self { content, backdrop, groups, stages })
    }

    pub fn version(&self) -> u64 { self.content.version() }

    pub fn pixels(&self) -> &[f32] {
        self.stages.last().map_or(self.content.raster().rgba, Vec::as_slice)
    }

    /// Apply a versioned batch and return its conservative output damage.
    /// Failed validation or rendering leaves the current raster and frame intact.
    pub fn update(&mut self, renderer: &mut Renderer, version: u64, patches: &[Patch<'_>]) -> Result<PixelRect> {
        let mut content = self.content.clone();
        content.update(version,patches)?;
        let damage = patches.iter().map(|p| p.rect).reduce(PixelRect::cover).unwrap();
        let prior = Prior { stages: &self.stages, damage };
        let mut stages = Vec::new();
        let (_,damage) = render_stages(renderer,&content,self.backdrop,self.groups,Some(prior),Some(&mut stages))?;
        self.content = content;
        self.stages = stages;
        Ok(damage)
    }
}
