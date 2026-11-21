"""Framework-independent experimental offscreen glass renderer (Linux/EGL).

Exposes recovered material passes, not a complete Apple compositor/render graph.
Input RGBA arrays and all material values are supplied by the caller.
"""
import math
from pathlib import Path
import numpy as np
if __package__:
    from .render_validation import Renderer, dimensions
else:
    from render_validation import Renderer, dimensions
if __package__:
    from .build_inline_sdf import BACKGROUND
else:
    from build_inline_sdf import BACKGROUND
if __package__:
    from .air_samplers import MATERIAL_FILTERS, MATERIAL_WRAPS
else:
    from air_samplers import MATERIAL_FILTERS, MATERIAL_WRAPS

def geometry_input(geometry,shape_transform):
    g=np.asarray(geometry,np.float32)
    if g.shape!=(3,4) or not np.isfinite(g).all():raise ValueError('geometry must be finite 3x4')
    mode=float(g[0,2])
    if abs(mode)>2147483000 or mode!=int(mode):raise ValueError('integer geometry mode required')
    if g[0,0]<=0 or g[0,1]<=0:raise ValueError('positive shape extents required')
    # Every background mode may request the rounded shadow/ring helper.
    if abs(int(mode))<=4 and g[2,2]==0:raise ValueError('zero radius is not validated')
    if abs(int(mode))>4 and np.any(g[2]==0):raise ValueError('zero corner radius is not validated')
    t=np.asarray(shape_transform,np.float32)
    if t.shape!=(2,3) or not np.isfinite(t).all():raise ValueError('explicit shape_transform 2x3 required')
    vertex='''#version 450
layout(location=0) out vec2 shape;
layout(location=1) out vec2 source;
layout(std140,binding=10) uniform ShapeTransform { vec4 row0; vec4 row1; } transform;
void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);
shape=vec2(dot(vec3(p,1),transform.row0.xyz),dot(vec3(p,1),transform.row1.xyz));
source=p;gl_Position=vec4(p*2-1,0,1);}
'''
    return g,vertex


class GlassRenderer:
    def __init__(self,size=128):
        self.width,self.height=dimensions(size)
        self.backend=Renderer(size)
        self.size=size
    def render(self,*,pass_name,source,material,edr,extension=None,sdf=None,geometry=None,shape_transform=None,mipmaps=None,sdf_transform=None):
        """Render with an optional scene-UV to SDF-texture-UV affine binding.

        Material uniforms, including shadow/ring offsets, stay in the caller's
        scene UV coordinates. The binding maps every SDF read, including those
        offsets and helper lanes, into the complete supplied texture domain.
        """
        if pass_name!='foreground' and pass_name not in BACKGROUND:raise ValueError('unknown material pass')
        foreground=pass_name=='foreground'
        required=80 if foreground else 320
        if len(material)!=required:raise ValueError(f'material must be {required} bytes, lpf layout')
        if not math.isfinite(edr):raise ValueError('finite EDR scale required')
        if not foreground and (extension is None or len(extension) not in (28,32)):
            raise ValueError('background extension must be explicit 28 or 32 bytes')
        if (geometry is None)==(sdf is None):raise ValueError('provide exactly one of geometry or sdf')
        def image(value):
            value=np.asarray(value,np.float32)
            if value.ndim!=3 or value.shape[2]!=4 or min(value.shape[:2])<1 or max(value.shape[:2])>4096 or not np.isfinite(value).all():
                raise ValueError('finite RGBA image required')
            return value
        source=image(source);vertex=None;extra={};fragment=None
        if geometry is not None:
            if sdf_transform is not None:raise ValueError('sdf_transform applies only to a supplied SDF texture')
            g,vertex=geometry_input(geometry,shape_transform)
            extra[10]=np.pad(np.asarray(shape_transform,np.float32),((0,0),(0,1))).tobytes()
            uniforms=g.tobytes()+bytes(material);sdf=np.zeros_like(source)
            filename='foreground-sdf.frag' if foreground else f'background-sdf-{pass_name}.frag'
        else:
            sdf=image(sdf);uniforms=bytes(material)
            if shape_transform is not None:raise ValueError('shape_transform applies only to inline geometry')
            filename='foreground.frag' if foreground else f'background-{pass_name}.frag'
            if sdf_transform is not None:
                binding=np.asarray(sdf_transform,np.float32)
                if binding.shape!=(2,3)or not np.isfinite(binding).all():raise ValueError('finite scene-UV to SDF-UV 2x3 affine required')
                extra[11]=np.pad(binding,((0,0),(0,1))).tobytes()
                # This resource adapter preserves the recovered material math;
                # do not patch displacement uniforms or crop the SDF texture.
                fragment=(Path(__file__).parent/filename).read_text()
                fragment=fragment.replace('texture(sdfTexture,','sampleBoundSdf(')
                marker='layout(binding=4) uniform sampler2D sdfTexture;'
                fragment=fragment.replace(marker,marker+'''
layout(std140,binding=11) uniform SDFBinding { vec4 row0; vec4 row1; } sdfBinding;
vec4 sampleBoundSdf(vec2 uv) {
    vec3 p=vec3(uv,1);
    return texture(sdfTexture,vec2(dot(sdfBinding.row0.xyz,p),dot(sdfBinding.row1.xyz,p)));
}
''')
        levels=[]
        for i,level in enumerate(mipmaps or [],1):
            level=np.asarray(level,np.float32)
            expected=(max(1,source.shape[0]>>i),max(1,source.shape[1]>>i),4)
            if level.shape!=expected or not np.isfinite(level).all():raise ValueError('invalid mip level')
            levels.append(level)
        ext=bytes(extension or b'').ljust(32,b'\0')
        result=self.backend.render(filename,source,sdf,uniforms,extension=ext,edr=edr,mipmaps=levels,vertex_source=vertex,wraps=MATERIAL_WRAPS,filters=MATERIAL_FILTERS,extra_uniforms=extra,fragment_source=fragment)
        if not np.isfinite(result).all():raise ValueError('nonfinite render: degenerate input or unvalidated shader case')
        return result
    def distance_field(self,geometry,shape_transform):
        g,vertex=geometry_input(geometry,shape_transform)
        dummy=np.zeros((1,1,4),np.float32)
        result=self.backend.render('sdf-probe.frag',dummy,dummy,g.tobytes(),vertex_source=vertex,extra_uniforms={10:np.pad(np.asarray(shape_transform,np.float32),((0,0),(0,1))).tobytes()})
        if not np.isfinite(result).all():raise ValueError('degenerate or overflowing SDF geometry')
        return result
    def close(self):self.backend.close()
    def __enter__(self):return self
    def __exit__(self,*_):self.close()
