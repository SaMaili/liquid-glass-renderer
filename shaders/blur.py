"""Recovered variable-blur fragment passes; schedule/sampler not Apple-validated."""
import math,struct
import numpy as np
if __package__:
    from .render_validation import Renderer, dimensions
else:
    from render_validation import Renderer, dimensions
if __package__:
    from .air_samplers import BLUR_FILTERS, BLUR_WRAPS, DOWNSAMPLE_SAMPLER
else:
    from air_samplers import BLUR_FILTERS, BLUR_WRAPS, DOWNSAMPLE_SAMPLER

def downsample(source,*,output_size,pixel_step,lod=0.0,bounds=None,mipmaps=None,backend_factory=Renderer):
    source=np.asarray(source,np.float32)
    if source.ndim!=3 or source.shape[2]!=4 or min(source.shape[:2])<1 or not np.isfinite(source).all():raise ValueError('finite RGBA image required')
    dimensions(output_size)
    if len(pixel_step)!=2 or not all(math.isfinite(v) and v>=0 for v in pixel_step):raise ValueError('two finite nonnegative sampling steps required')
    if not math.isfinite(lod) or lod<0:raise ValueError('nonnegative finite LOD required')
    if bounds is not None:
        if len(bounds)!=4 or not all(math.isfinite(v) for v in bounds) or bounds[0]>bounds[2] or bounds[1]>bounds[3]:raise ValueError('ordered finite bounds required')
        if lod!=0 or mipmaps:raise ValueError('clamp path currently supports base-level source only')
    for i,level in enumerate(mipmaps or [],1):
        level=np.asarray(level)
        if level.shape!=(max(1,source.shape[0]>>i),max(1,source.shape[1]>>i),4) or not np.isfinite(level).all():raise ValueError('invalid mip level')
    uniforms=struct.pack('<8f',lod,*pixel_step,0,*(bounds or (0,0,1,1)))
    shader='blur-downsample-clamp.frag' if bounds is not None else 'blur-downsample.frag'
    with backend_factory(output_size) as r:
        return r.render(shader,source,np.zeros((1,1,4),np.float32),uniforms,mipmaps=mipmaps,wraps=(DOWNSAMPLE_SAMPLER["wrap"],"clamp","clamp"),filters=(DOWNSAMPLE_SAMPLER["filters"],)*3)

def pyramid(source,*,pixel_steps):
    """Caller-chosen half-resolution schedule. Not a recovered Apple scheduler.

    Each explicit pair sets the sampling step in that level's source UV space.
    Returns only reduced levels; the caller retains the original base texture.
    """
    current=np.asarray(source,np.float32);result=[]
    if current.ndim!=3 or current.shape[2]!=4:raise ValueError('RGBA source required for pyramid')
    for step in pixel_steps:
        if max(current.shape[:2])<=1:raise ValueError('too many levels')
        current=downsample(current,output_size=(max(1,current.shape[1]//2),max(1,current.shape[0]//2)),pixel_step=step)
        result.append(current)
    return result

def resolve(source, mask, *, uniforms, mipmaps=None, edr=1.0, noise_texture=None, noise_strength=0.0, source_transform=(1,1,0,0), mask_transform=(1,1,0,0), source_matrix=None,mask_matrix=None,wraps=BLUR_WRAPS, output_size=None,backend_factory=Renderer):
    """AIR 0037 variable blur, including explicit optional noise.

    uniforms: original 12 floats, preserving AIR field order. The first field
    is the radius multiplier, fields 4/5 are UV offsets per LOD. Caller supplies
    all values: no inferred Apple defaults. Optional row-major 2x3 UV matrices
    preserve swapped/rotated native projections; they override diagonal maps.
    """
    source=np.asarray(source,np.float32);mask=np.asarray(mask,np.float32)
    for image in (source,mask):
        if image.ndim!=3 or image.shape[2]!=4 or min(image.shape[:2])<1 or max(image.shape[:2])>4096 or not np.isfinite(image).all():
            raise ValueError('finite RGBA source and mask required')
    target=(source.shape[1],source.shape[0]) if output_size is None else dimensions(output_size)
    p=np.asarray(uniforms,np.float32)
    if p.shape!=(12,) or not np.isfinite(p).all() or p[0]<0 or p[8]<0:raise ValueError('12 finite uniforms; nonnegative radii required')
    if p[1] not in (0,1):raise ValueError('noise flag must be 0 or 1')
    if not math.isfinite(noise_strength):raise ValueError('finite noise strength required')
    if p[1]:
        noise_texture=np.asarray(noise_texture,np.float32)
        if noise_texture.ndim!=3 or noise_texture.shape[2]!=4 or min(noise_texture.shape[:2])<1 or not np.isfinite(noise_texture).all():raise ValueError('finite RGBA noise texture required')
    if not math.isfinite(edr):raise ValueError('finite EDR required')
    for i,level in enumerate(mipmaps or [],1):
        if np.asarray(level).shape!=(max(1,source.shape[0]>>i),max(1,source.shape[1]>>i),4) or not np.isfinite(level).all():raise ValueError('invalid mip chain')
    transforms=[]
    for transform,matrix in ((source_transform,source_matrix),(mask_transform,mask_matrix)):
        transform=np.asarray(transform,np.float32)
        if transform.shape!=(4,) or not np.isfinite(transform).all():raise ValueError('finite UV scale.xy/offset.xy required')
        if matrix is None:
            sx,sy,ox,oy=transform;matrix=np.array(((sx,0,ox),(0,sy,oy)),np.float32)
        else:matrix=np.asarray(matrix,np.float32)
        if matrix.shape!=(2,3)or not np.isfinite(matrix).all():raise ValueError('finite row-major 2x3 UV matrix required')
        transforms.extend((*row,0.)for row in matrix)
    vertex='''#version 450
layout(location=0) out vec2 sourceUV;
layout(location=1) out vec2 sdfUV;
layout(std140,binding=10) uniform UVRegistration {vec4 rows[4];} maps;
void main(){
 vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);vec3 q=vec3(p,1);
 sourceUV=vec2(dot(maps.rows[0].xyz,q),dot(maps.rows[1].xyz,q));
 sdfUV=vec2(dot(maps.rows[2].xyz,q),dot(maps.rows[3].xyz,q));
 gl_Position=vec4(p*2.0-1.0,0,1);
}
'''
    with backend_factory(target) as r:
        out=r.render('variable-blur.frag',source,mask,p.tobytes(),edr=edr,mipmaps=mipmaps,noise_texture=noise_texture,vertex_source=vertex,wraps=wraps,filters=BLUR_FILTERS,extension=struct.pack('<8f',noise_strength,0,0,0,0,0,0,0),extra_uniforms={10:np.asarray(transforms,'<f4').tobytes()})
    if not np.isfinite(out).all():raise ValueError('nonfinite blur output')
    return out

def render_pipeline(source, mask, *, uniforms, pixel_steps, edr=1.0, noise_texture=None, noise_strength=0.0, source_transform=(1,1,0,0), mask_transform=(1,1,0,0), wraps=BLUR_WRAPS):
    """Executable build+resolve adapter, with explicit unrecovered tap schedule."""
    levels=pyramid(source,pixel_steps=pixel_steps)
    return resolve(source,mask,uniforms=uniforms,mipmaps=levels,edr=edr,noise_texture=noise_texture,noise_strength=noise_strength,source_transform=source_transform,mask_transform=mask_transform,wraps=wraps)

def agx2_pyramid(source, *, level_count,backend_factory=Renderer):
    """Reconstructed AGX2 per-pixel kernel and iterative level dimensions.

    Direct fetches emulate the threadgroup cache. Base-level copy, cropping,
    native sampler state and hardware fast-math contraction remain unverified.
    """
    current=np.asarray(source,np.float32)
    if current.ndim!=3 or current.shape[2]!=4 or min(current.shape[:2])<1 or max(current.shape[:2])>4096 or not np.isfinite(current).all():raise ValueError('finite RGBA source required')
    capacity=max(current.shape[:2]).bit_length()
    if type(level_count)!=int or not 1<=level_count<=capacity:raise ValueError('invalid level count')
    levels=[]
    for _ in range(1,level_count):
        size=(max(1,current.shape[1]//2),max(1,current.shape[0]//2))
        with backend_factory(size) as r:
            current=r.render('blur-agx2.frag',current,np.zeros((1,1,4),np.float32),struct.pack('<4f',1/size[0],1/size[1],0,0),filters=(BLUR_FILTERS[1],)*3)
        if not np.isfinite(current).all():raise ValueError('half overflow in blur')
        levels.append(current)
    return levels

def planned_pipeline(source,mask,*,s0,s1,s2,flag,uniforms,edr=1.0,noise_texture=None,noise_strength=0.0, source_transform=(1,1,0,0), mask_transform=(1,1,0,0), wraps=BLUR_WRAPS):
    """Full-image host composition of recovered planner, AGX2 kernel and resolve.

    Uniforms still explicit; this is not the original cropped Metal rendergraph.
    """
    if __package__:
        from .blur_levels import parameters
    else:
        from blur_levels import parameters
    source=np.asarray(source,np.float32)
    if source.ndim!=3:raise ValueError('RGBA source required')
    h,w=source.shape[:2]
    plan=parameters(w,h,bounds=(0,0,w,h),s0=s0,s1=s1,s2=s2,flag=flag)
    levels=agx2_pyramid(source,level_count=plan['level_count'])
    image=resolve(source,mask,uniforms=uniforms,mipmaps=levels,edr=edr,noise_texture=noise_texture,noise_strength=noise_strength,source_transform=source_transform,mask_transform=mask_transform,wraps=wraps)
    return image,plan
