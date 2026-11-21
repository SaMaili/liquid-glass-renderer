"""Explicit scene composition of recovered passes.

The caller supplies order, merge groups and material values. This host graph
does not claim to reconstruct Apple's unresolved automatic contour matching.
Strict union errors are preserved rather than inventing native NaN semantics.
"""
from dataclasses import dataclass,replace
from collections.abc import Mapping
import math,copy
import numpy as np
if __package__:
    from .renderer import GlassRenderer,BACKGROUND
else:
    from renderer import GlassRenderer,BACKGROUND
if __package__:
    from .render_validation import dimensions
else:
    from render_validation import dimensions
if __package__:
    from .pool import RendererPool
else:
    from pool import RendererPool
if __package__:
    from .morph import union_gpu
else:
    from morph import union_gpu
if __package__:
    from .blur import agx2_pyramid,resolve
else:
    from blur import agx2_pyramid,resolve
if __package__:
    from .blur_levels import parameters
else:
    from blur_levels import parameters
if __package__:
    from .color import convert,over,rgba
else:
    from color import convert,over,rgba
if __package__:
    from .fluid import FluidAnimator,FluidSpring
else:
    from fluid import FluidAnimator,FluidSpring


@dataclass(frozen=True)
class Material:
    pass_name: str
    uniforms: bytes
    extension: bytes | None = None
    edr: float = 1.

    def __post_init__(self):
        if self.pass_name!='foreground' and self.pass_name not in BACKGROUND:raise ValueError('unknown material pass')
        object.__setattr__(self,'uniforms',bytes(self.uniforms))
        if self.extension is not None:object.__setattr__(self,'extension',bytes(self.extension))
        if len(self.uniforms)!=(80 if self.pass_name=='foreground' else 320):raise ValueError('incorrect lpf uniform size')
        if self.pass_name!='foreground' and (self.extension is None or len(self.extension) not in (28,32)):raise ValueError('explicit background extension required')
        if not math.isfinite(self.edr):raise ValueError('finite EDR required')

    @classmethod
    def from_values(cls,pass_name,values,*,extension_values=None,edr=1.):
        if __package__:
            from .uniforms import pack
        else:
            from uniforms import pack
        foreground=pass_name=='foreground'
        return cls(pass_name,pack(values,'foreground' if foreground else 'background'),
                   None if foreground else pack(extension_values,'background_ext'),edr)


@dataclass(frozen=True)
class Element:
    identifier: str
    material: Material
    geometry: object = None
    transform: object = None
    field: object = None
    group: str | None = None
    opacity: float = 1.

    def __post_init__(self):
        if not isinstance(self.identifier,str) or not self.identifier:raise ValueError('nonempty stable ID required')
        if (self.geometry is None)==(self.field is None):raise ValueError('one geometry or field required')
        if not math.isfinite(self.opacity) or not 0<=self.opacity<=1:raise ValueError('opacity must be in [0,1]')

    @classmethod
    def rectangle(cls,identifier,material,frame,radius,*,canvas_size,group=None,corner_blend=(1.,1.)):
        if __package__:
            from .shape import rectangle_geometry
        else:
            from shape import rectangle_geometry
        cw,ch=dimensions(canvas_size)
        if len(frame)!=4 or any(not math.isfinite(v) for v in frame) or min(frame[2:])<=0:
            raise ValueError('positive finite rectangle required')
        radii=(float(radius),)*4 if np.isscalar(radius) else tuple(radius)
        if len(radii)!=4 or any(not math.isfinite(v) for v in radii):raise ValueError('one or four finite radii required')
        if len(corner_blend)!=2 or any(not math.isfinite(v) or not 0<=v<=1 for v in corner_blend):raise ValueError('two corner blends in [0,1] required')
        x,y,w,h=frame
        geometry=rectangle_geometry((w,h),radii)
        if geometry[0,2]==4:geometry[2,:2]=corner_blend
        elif tuple(corner_blend)!=(1.,1.):raise ValueError('custom blend is only defined for uniform radii')
        return cls(identifier,material,geometry=geometry,
                   transform=[[cw,0,-x-w/2],[0,ch,-y-h/2]],group=group)


@dataclass(frozen=True)
class BlurConfig:
    s0: float
    s1: float
    s2: float
    flag: bool
    uniforms: tuple | None = None


class SceneRenderer:
    def __init__(self,size,*,max_blur_contexts=8):
        self.width,self.height=dimensions(size)
        self.material_renderer=GlassRenderer(size)
        self.pool=RendererPool(max_blur_contexts)
        self.closed=False
        self.last_trace=[]

    def render(self,backdrop,elements,*,smoothness,backdrop_mode,
               blur=None,blur_mask=None,source_transfer='linear',source_alpha='premultiplied',
               output_transfer='linear',output_alpha='premultiplied',group_compositor=None,group_smoothness=None):
        if self.closed:raise RuntimeError('scene renderer is closed')
        if backdrop_mode not in ('original','accumulated'):raise ValueError('explicit backdrop mode required')
        if not math.isfinite(smoothness) or smoothness<0:raise ValueError('nonnegative finite smoothness required')
        image=convert(backdrop,source_transfer=source_transfer,source_alpha=source_alpha)
        if image.shape[:2]!=(self.height,self.width):raise ValueError('backdrop must match scene output')
        if group_smoothness is not None and(not isinstance(group_smoothness,Mapping)or any(not math.isfinite(v)or v<0 for v in group_smoothness.values())):raise ValueError('finite nonnegative per-group smoothness mapping required')
        def blend_for(group):return smoothness if group_smoothness is None else group_smoothness.get(group[0].group,smoothness)
        ordered=list(elements)
        if len({e.identifier for e in ordered})!=len(ordered):raise ValueError('duplicate element ID')
        self.last_trace=[]
        groups=[]
        for element in ordered:
            if groups and element.group is not None and groups[-1][-1].group == element.group:
                groups[-1].append(element)
            else:
                groups.append([element])
        for group in groups:
            if any(e.material!=group[0].material or e.opacity!=group[0].opacity for e in group[1:]):raise ValueError('merge group requires identical resolved material and opacity; native container material resolution is not inferred')
        original=image.copy()
        cached_original=None
        for group in groups:
            field = None
            for e in group:
                if e.field is not None:
                    current=np.asarray(e.field,np.float32)
                    if current.shape!=(self.height,self.width,4) or not np.isfinite(current).all():raise ValueError('matching finite scene SDF required')
                else:
                    current=self.material_renderer.distance_field(e.geometry,e.transform)
                self.last_trace.append({'pass':'sdf','element':e.identifier})
                if field is None:field=current
                elif not np.array_equal(field,current):
                    field=union_gpu(field,current,smoothness=blend_for(group),backend_factory=self.pool.acquire)
                    self.last_trace.append({'pass':'union','element':e.identifier,'smoothness':blend_for(group)})
            source=original if backdrop_mode=='original' else image
            levels=[]
            if blur is not None:
                if backdrop_mode=='original' and cached_original is not None:
                    source,levels=cached_original
                else:
                    h,w=source.shape[:2]
                    if not isinstance(blur, BlurConfig):
                        raise ValueError('BlurConfig required')
                    plan=parameters(w,h,bounds=(0,0,w,h),s0=blur.s0,s1=blur.s1,s2=blur.s2,flag=blur.flag)
                    levels=agx2_pyramid(source,level_count=plan['level_count'],backend_factory=self.pool.acquire)
                    self.last_trace.append({'pass':'blur-pyramid','levels':len(levels)+1})
                    if blur.uniforms is not None:
                        mask=np.ones_like(source) if blur_mask is None else blur_mask
                        source=resolve(source,mask,uniforms=blur.uniforms,mipmaps=levels,backend_factory=self.pool.acquire)
                        levels=[]
                        self.last_trace.append({'pass':'blur-resolve'})
                    if backdrop_mode=='original':cached_original=(source,levels)
            e=group[0];m=e.material
            layer=self.material_renderer.render(pass_name=m.pass_name,source=source,sdf=field,
                material=m.uniforms,extension=m.extension,edr=m.edr,mipmaps=levels)
            image=over(layer*e.opacity,image)
            self.last_trace.append({'pass':'material-over','variant':m.pass_name,'elements':[e.identifier for e in group]})
            if group_compositor is not None:
                image=group_compositor(image,tuple(group))
                if image.shape!=(self.height,self.width,4)or not np.isfinite(image).all():raise ValueError('group compositor must return a finite matching linear premultiplied image')
        return convert(image,target_transfer=output_transfer,target_alpha=output_alpha)

    def close(self):
        if self.closed:return
        self.material_renderer.close();self.pool.close();self.closed=True
    def __enter__(self):return self
    def __exit__(self,*_):self.close()


class ElementAnimator:
    """Explicit stable-ID geometry tracking; topology changes are rejected.

    Shapes and their affine transforms use the recovered Euclidean spring.
    The host supplies correspondence; this is not Apple's path-matching rule.
    """
    def __init__(self,elements,*,time=0.):
        self.items={};self.order=[];self.last_time=time
        self.set_elements(elements,time=time,spring=FluidSpring())

    def set_elements(self,elements,*,time,spring):
        if not math.isfinite(time) or time<self.last_time:raise ValueError('monotonic time required')
        elements=list(elements)
        if len({e.identifier for e in elements})!=len(elements):raise ValueError('duplicate IDs')
        prepared={}
        for e in elements:
            if e.geometry is None:raise ValueError('geometry tracking requires explicit geometry')
            g=np.asarray(e.geometry,np.float64);t=np.asarray(e.transform,np.float64)
            if g.shape!=(3,4) or t.shape!=(2,3) or not np.isfinite(g).all() or not np.isfinite(t).all():raise ValueError('finite geometry and transform required')
            value=np.r_[g.ravel(),t.ravel()]
            if e.identifier in self.items:
                old,previous_animator=self.items[e.identifier]
                animator=copy.deepcopy(previous_animator)
                if np.asarray(old.geometry)[0,2]!=g[0,2]:raise ValueError('geometry mode change has no recovered correspondence')
                animator.retarget(value,time=time,spring=spring)
            else:animator=FluidAnimator(value,value,spring=spring,time=time)
            prepared[e.identifier]=(e,animator)
        self.items=prepared;self.order=[e.identifier for e in elements];self.last_time=time

    def sample(self,time):
        if not math.isfinite(time) or time<self.last_time:raise ValueError('monotonic time required')
        self.last_time=time;result=[]
        for identifier in self.order:
            e,animator=self.items[identifier];value=animator.sample(time)
            g=value[:12].reshape(3,4);t=value[12:].reshape(2,3)
            result.append(replace(e,geometry=g,transform=t))
        return result
