use liquid_glass_core::*;
const COPY:&str="#version 450\nlayout(location=0) in vec2 sourceUV;layout(location=0) out vec4 color;layout(binding=3) uniform sampler2D image;void main(){color=texture(image,sourceUV);}";
fn copy<'a>(size:Size,pixels:&'a [f32])->Pass<'a>{
    Pass{fragment:COPY,vertex:None,source:TextureInput{image:Raster{size,rgba:pixels},mipmaps:&[],sampler:Sampler::default()},sdf:TextureInput{image:Raster{size,rgba:pixels},mipmaps:&[],sampler:Sampler::default()},noise:None,uniforms:&[0;16],extension:&[],edr:1.,extra_uniforms:&[],vertex_count:3}
}
#[test]
fn reuse_invalid_inputs_and_independent_contexts(){
    let size=Size::new(16,12).unwrap();let pixels=vec![0.25;size.pixels()*4];
    let mut a=Renderer::new(size).unwrap();let pass=copy(size,&pixels);
    assert_eq!(a.render(&pass).unwrap(),pixels);let warm=a.stats();
    for _ in 0..100{assert_eq!(a.render(&pass).unwrap(),pixels);}
    let final_stats=a.stats();
    assert_eq!(warm.texture_allocations,final_stats.texture_allocations);
    assert_eq!(warm.buffer_allocations,final_stats.buffer_allocations);
    assert_eq!(warm.resident_texture_bytes,final_stats.resident_texture_bytes);
    assert_eq!(final_stats.program_compilations,1);assert_eq!(final_stats.program_hits,100);
    let count=a.stats().draws;
    let mut bad=copy(size,&pixels);bad.extra_uniforms=&[(1,&[0;16])];assert!(a.render(&bad).is_err());
    bad=copy(size,&pixels);bad.source.image.rgba=&pixels[..4];assert!(a.render(&bad).is_err());
    bad=copy(size,&pixels);bad.edr=f32::NAN;assert!(a.render(&bad).is_err());assert_eq!(a.stats().draws,count);
    for _ in 0..8{let mut b=Renderer::new(size).unwrap();assert_eq!(b.render(&pass).unwrap(),pixels);drop(b);assert_eq!(a.render(&pass).unwrap(),pixels);}
}
#[test]
fn bounded_texture_storage_and_failed_shader_recovery(){
    let size=Size::new(16,16).unwrap();let pixels=vec![0.5;size.pixels()*4];
    let mut r=Renderer::with_budget(size,size.pixels()*16*3).unwrap();
    let pass=copy(size,&pixels);assert_eq!(r.render(&pass).unwrap(),pixels);
    let mut bad=copy(size,&pixels);bad.fragment="#version 450\ninvalid syntax";assert!(r.render(&bad).is_err());
    assert_eq!(r.render(&pass).unwrap(),pixels);
    let large=Size::new(64,64).unwrap();let big=vec![0.5;large.pixels()*4];let bad=copy(large,&big);
    let stats=r.stats();assert!(r.render(&bad).is_err());assert_eq!(r.stats().resident_texture_bytes,stats.resident_texture_bytes);assert_eq!(r.stats().draws,stats.draws);
    assert_eq!(r.render(&pass).unwrap(),pixels);
}

#[test]
fn alternating_pass_footprints_reuse_textures_and_uniform_capacity() {
    let target = Size::new(16,16).unwrap();
    let small = Size::new(8,8).unwrap();
    let large_pixels = vec![0.25;target.pixels()*4];
    let small_pixels = vec![0.5;small.pixels()*4];
    let mip = [Raster {size:small,rgba:&small_pixels}];
    let uniform_shader = "#version 450\nlayout(location=0) in vec2 sourceUV;\
        layout(location=0) out vec4 color;layout(binding=3) uniform sampler2D image;\
        layout(std140,binding=1) uniform Add {vec4 value;} u;\
        void main(){color=textureLod(image,sourceUV,0.0)+u.value;}";
    let short = 0.125f32.to_ne_bytes().repeat(4);
    let long = 0.25f32.to_ne_bytes().repeat(80);
    let mut a = copy(target,&large_pixels);
    a.fragment = uniform_shader;
    a.source.mipmaps = &mip;
    a.uniforms = &long;
    let mut b = copy(small,&small_pixels);
    b.fragment = uniform_shader;
    b.uniforms = &short;
    let budget = 24*1024;
    let mut renderer = Renderer::with_budget(target,budget).unwrap();

    for _ in 0..2 {
        assert_eq!(renderer.render(&a).unwrap(), vec![0.5;target.pixels()*4]);
        assert_eq!(renderer.render(&b).unwrap(), vec![0.625;target.pixels()*4]);
    }
    let warm = renderer.stats();
    for _ in 0..64 {
        assert_eq!(renderer.render(&a).unwrap(), vec![0.5;target.pixels()*4]);
        assert_eq!(renderer.render(&b).unwrap(), vec![0.625;target.pixels()*4]);
    }
    let stats = renderer.stats();
    assert_eq!(stats.texture_allocations,warm.texture_allocations);
    assert_eq!(stats.buffer_allocations,warm.buffer_allocations);
    assert_eq!(stats.resident_texture_bytes,warm.resident_texture_bytes);
    assert_eq!(stats.resident_buffer_bytes,warm.resident_buffer_bytes);
    assert_eq!(stats.texture_pool_hits-warm.texture_pool_hits,64*4);
    assert!(stats.resident_texture_bytes <= budget);

    // A second owner can hold the live large pass but cannot retain every
    // previous input. Eviction and recreation must preserve pixel contents.
    let tight_budget = (3*target.pixels()+small.pixels())*16;
    let mut limited = Renderer::with_budget(target,tight_budget).unwrap();
    for _ in 0..8 {
        assert_eq!(limited.render(&a).unwrap(), vec![0.5;target.pixels()*4]);
        assert_eq!(limited.render(&b).unwrap(), vec![0.625;target.pixels()*4]);
        assert!(limited.stats().resident_texture_bytes <= tight_budget);
    }
    assert!(limited.stats().texture_evictions>0);
}
