//! Feed the controller's pointer events into the full scene and overlay pipeline.
use liquid_glass_core::{
    Renderer, Stats,
    frame::{CacheStats, Frame, FrameCache, FrameStage, LightSpot},
    interaction::{Glow, Input, controller::{Controller, Presentation}},
    material::{BackgroundStyle, recipe::{RecipeRole, SdrRecipe}, spec::Dimensions},
    overlay::Appearance,
    raster::{Scale, Viewport},
    scene::{Backdrop, Group, MaterialKind},
    shape::{Rectangle, affine_rectangle},
};
use std::{
    ffi::c_long,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    time::Instant,
};

#[repr(C)]
struct Timespec {
    seconds: c_long,
    nanoseconds: c_long,
}

unsafe extern "C" {
    fn clock_gettime(clock: i32, value: *mut Timespec) -> i32;
}

fn process_time_ns() -> io::Result<u128> {
    const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
    let mut time = Timespec { seconds: 0, nanoseconds: 0 };
    if unsafe { clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &mut time) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(time.seconds as u128 * 1_000_000_000 + time.nanoseconds as u128)
}

struct Measurement {
    renderer: Stats,
    cache: CacheStats,
    draw_start_ns: u64,
    ticks: u64,
    cpu_ns: u128,
    wall_ns: u128,
    peak_texture_bytes: usize,
    peak_buffer_bytes: usize,
    peak_cache_bytes: usize,
}

impl Measurement {
    fn start(renderer: &Renderer, cache: &FrameCache) -> Self {
        let renderer_stats = renderer.stats();
        let cache = cache.stats();
        Self {
            renderer: renderer_stats,
            cache,
            draw_start_ns: renderer.draw_time_ns(),
            ticks: 0,
            cpu_ns: 0,
            wall_ns: 0,
            peak_texture_bytes: renderer_stats.resident_texture_bytes,
            peak_buffer_bytes: renderer_stats.resident_buffer_bytes,
            peak_cache_bytes: cache.resident_bytes,
        }
    }

    fn record(&mut self, renderer: &Renderer, cache: &FrameCache, start: (Instant, u128)) -> io::Result<()> {
        self.cpu_ns += process_time_ns()? - start.1;
        self.wall_ns += start.0.elapsed().as_nanos();
        self.ticks += 1;
        let stats = renderer.stats();
        self.peak_texture_bytes = self.peak_texture_bytes.max(stats.resident_texture_bytes);
        self.peak_buffer_bytes = self.peak_buffer_bytes.max(stats.resident_buffer_bytes);
        self.peak_cache_bytes = self.peak_cache_bytes.max(cache.stats().resident_bytes);
        Ok(())
    }

    fn report(&self, renderer: &Renderer, cache: &FrameCache) {
        let stats = renderer.stats();
        let cache = cache.stats();
        eprintln!(
            concat!(
                "{{\"ticks\":{},\"cpu_ns\":{},\"wall_ns\":{},\"gl_draw_ns\":{},",
                "\"draws\":{},\"texture_allocations\":{},\"texture_evictions\":{},",
                "\"buffer_allocations\":{},\"program_compilations\":{},\"texture_hits\":{},",
                "\"program_hits\":{},\"frame_hits\":{},\"prefix_stages\":{},",
                "\"peak_texture_bytes\":{},\"peak_buffer_bytes\":{},\"peak_cache_bytes\":{}}}"
            ),
            self.ticks, self.cpu_ns, self.wall_ns, renderer.draw_time_ns() - self.draw_start_ns,
            stats.draws - self.renderer.draws, stats.texture_allocations - self.renderer.texture_allocations,
            stats.texture_evictions - self.renderer.texture_evictions,
            stats.buffer_allocations - self.renderer.buffer_allocations,
            stats.program_compilations - self.renderer.program_compilations,
            stats.texture_pool_hits - self.renderer.texture_pool_hits, stats.program_hits - self.renderer.program_hits,
            cache.hits - self.cache.hits, cache.reused_stages - self.cache.reused_stages,
            self.peak_texture_bytes, self.peak_buffer_bytes, self.peak_cache_bytes,
        );
    }
}

const WIDTH: u32 = 288;
const HEIGHT: u32 = 176;
const CENTER: [f64;2] = [112.137,110.193];
const REST: [f64;2] = [180.0,76.0];

fn rect(center: [f64;2], size: [f64;2], corner: f64) -> Rectangle {
    Rectangle {
        origin: std::array::from_fn(|i| center[i]-size[i]/2.0),
        size, radii: [corner;4], continuous: true,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scale = match std::env::args().nth(1).as_deref() {
        Some("1") => Scale::One,
        Some("2") => Scale::Two,
        _ => return Err("expected scale 1 or 2".into()),
    };
    let viewport = Viewport::new(WIDTH,HEIGHT,scale)?;
    let mut input = BufReader::new(io::stdin().lock());
    let mut raw = vec![0;viewport.pixels().pixels()*16];
    input.read_exact(&mut raw)?;
    let pixels: Vec<[f32;4]> = raw.chunks_exact(16).map(|pixel| std::array::from_fn(|i| {
        f32::from_le_bytes(pixel[i*4..i*4+4].try_into().unwrap())
    })).collect();
    let source = Frame::new(viewport,pixels)?;
    let uncached = std::env::args().skip(2).any(|arg| arg == "--uncached");
    let profile = std::env::args().skip(2).any(|arg| arg == "--profile");
    let budget = if uncached { 0 } else { 16*1024*1024 };
    let mut cache = FrameCache::new(budget);
    let mut renderer = Renderer::new(viewport.pixels())?;
    if profile {
        eprintln!("device={}", renderer.device()?);
        renderer.enable_draw_timing()?;
    }
    let mut measurement = None;
    let mut output = BufWriter::new(io::stdout().lock());
    let mut controller = Controller::new(REST)?;
    let mut size = REST;
    let mut last = Presentation {
        scale:[1.0;2], translation:[0.0;2], needs_frame:false,
        glow:Glow {opacity:0.0,big_opacity:0.0,scale:1.0,diameter:0.0,position:[0.0;2]},
    };
    let appearance = Appearance::Dark;
    let recipe = |role, height, key_fill_amount| SdrRecipe {
        role, appearance,
        dimensions:Dimensions {minimum:Some(height),largest_area_minimum:Some(height)},
        diffusion:0.5, tint:None, style:BackgroundStyle {refraction_gain:1.0,key_fill_amount},
    }.build(viewport);
    let clear = recipe(RecipeRole::Clear,58.0,None)?;
    let regular = recipe(RecipeRole::Regular,96.0,None)?;
    let search = recipe(RecipeRole::Regular,28.0,Some(0.38))?;
    let fixed = |rectangle| affine_rectangle(viewport,rectangle,[1.0,0.0,0.0,1.0,0.0,0.0]);
    let clear_shapes = [fixed(rect([81.137,72.193],[102.0,58.0],22.0))?];
    let search_shapes = [fixed(rect([165.137,33.193],[124.0,28.0],14.0))?];

    for line in input.lines() {
        let line = line?;
        let mut words = line.split_whitespace();
        match words.next() {
            Some("hover") => {
                let hovered = match words.next() {
                    Some("enter") => true,
                    Some("leave") => false,
                    _ => return Err("expected hover enter or leave".into()),
                };
                controller.set_hovered(hovered);
            }
            Some("press") => {
                let kind = match words.next() {
                    Some("cursor") => Input::Cursor,
                    Some("touch") => Input::Touch,
                    _ => return Err("expected input kind".into()),
                };
                controller.press(kind, pair(&mut words)?)?;
            }
            Some("drag") => controller.drag(pair(&mut words)?)?,
            Some("resize") => { size = pair(&mut words)?; controller.resize(size)?; }
            Some("release") => controller.release(),
            Some("cancel") => controller.cancel(),
            Some("reduced") => controller.set_reduced_motion(words.next()==Some("1")),
            Some("highlights") => controller.set_reduced_highlighting(words.next()==Some("1")),
            Some("measure") if profile && measurement.is_none() => {
                measurement = Some(Measurement::start(&renderer, &cache));
            }
            Some("tick") => {
                let time = words.next().ok_or("missing clock value")?.parse()?;
                let force = words.next()==Some("1");
                let companion_x = words.next().ok_or("missing companion position")?.parse()?;
                let start = if measurement.is_some() { Some((Instant::now(), process_time_ns()?)) } else { None };
                let sampled = controller.sample(time)?;
                let changed = sampled.is_some();
                if let Some(sampled) = sampled { last = sampled; }
                let frame = if changed || force {
                    let rectangle = rect(CENTER,size,0.48*size[0].min(size[1]));
                    let moving = last.rectangle(viewport,rectangle)?;
                    let companion = fixed(rect([companion_x,109.601],[72.0,54.0],23.0))?;
                    let shapes = [companion,moving];
                    let clear_groups = [
                        Group {shapes:&clear_shapes,material:clear.material.material(MaterialKind::All)?,
                               shape_tags:&[],tints:&[],
                               opacity:1.0,smoothness:0.0,blur_levels:clear.blur_levels},
                    ];
                    let regular_groups = [
                        Group {shapes:&shapes,material:regular.material.material(MaterialKind::All)?,
                               shape_tags:&[],tints:&[],
                               opacity:1.0,smoothness:18.0,blur_levels:regular.blur_levels},
                    ];
                    let fill = controller.fill();
                    let glow = &last.glow;
                    let spot = (glow.opacity>0.0 || glow.big_opacity>0.0)
                        .then_some(LightSpot { glow, origin:rectangle.origin });
                    let search_groups = [
                        Group {shapes:&search_shapes,material:search.material.material(MaterialKind::All)?,
                               shape_tags:&[],tints:&[],
                               opacity:1.0,smoothness:0.0,blur_levels:search.blur_levels},
                    ];
                    Some(cache.render(&mut renderer,&source,&[
                        FrameStage::Glass {backdrop:Backdrop::Original,groups:&clear_groups},
                        FrameStage::Glass {backdrop:Backdrop::Original,groups:&regular_groups},
                        FrameStage::Interaction {shape:moving,appearance,fill,light:spot},
                        FrameStage::Glass {backdrop:Backdrop::Original,groups:&search_groups},
                    ])?)
                } else {
                    None
                };
                if let (Some(measurement), Some(start)) = (&mut measurement, start) {
                    measurement.record(&renderer, &cache, start)?;
                }
                output.write_all(&[u8::from(changed)])?;
                if let Some(frame) = frame {
                    for pixel in frame.pixels() {
                        for v in pixel { output.write_all(&v.to_le_bytes())?; }
                    }
                }
                output.flush()?;
            }
            _ => return Err("unsupported animation fixture row".into()),
        }
    }
    if profile {
        measurement.ok_or("missing measurement start")?.report(&renderer, &cache);
        return Ok(());
    }
    let stats=cache.stats();
    eprintln!(
        "{{\"frame_hits\":{},\"prefix_stages\":{},\"resident_bytes\":{},\"draws\":{}}}",
        stats.hits,stats.reused_stages,stats.resident_bytes,renderer.stats().draws
    );
    Ok(())
}

fn pair(words: &mut std::str::SplitWhitespace<'_>) -> Result<[f64;2],Box<dyn std::error::Error>> {
    let read = |words: &mut std::str::SplitWhitespace<'_>| -> Result<f64,Box<dyn std::error::Error>> {
        Ok(words.next().ok_or("missing coordinate")?.parse()?)
    };
    Ok([read(words)?,read(words)?])
}
