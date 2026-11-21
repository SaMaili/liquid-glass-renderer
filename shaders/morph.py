"""Packed SDFUnion software path reconstructed from 26A434 QuartzCore.

Input/output: little-endian signed int16 distance, signed int8 gx, gy.
This is the software blend, not a claim about the GPU or animation scheduler.
Degenerate 0/0 blend weights and half overflow are deliberately unsupported.
"""
import numpy as np

PIXEL = np.dtype([('distance','<i2'),('gx','i1'),('gy','i1')])

def union(a, b, *, smoothness):
    """Blend equally sized packed SDF byte buffers, preserving half roundings.

    smoothness is the float32 scalar passed to the original blend function.
    Returns bytes in the same packed format. No gradient normalization.
    """
    if len(a)!=len(b) or len(a)%4:raise ValueError('equal packed SDF buffers required')
    if not np.isfinite(smoothness) or not 0<=smoothness<=65504:raise ValueError('smoothness must fit finite positive half precision')
    if not len(a):return b''
    def h(x):return np.asarray(x,dtype=np.float16).astype(np.float64)
    def f(x):return np.asarray(x,dtype=np.float32).astype(np.float64)
    aa=np.frombuffer(a,PIXEL);bb=np.frombuffer(b,PIXEL)
    scale=float(np.array([0x2008],np.uint16).view(np.float16)[0])
    with np.errstate(over='ignore',invalid='ignore',divide='ignore'):
        da=h(h(aa['distance'])*scale);db=h(h(bb['distance'])*scale)
        ga=h(np.stack([aa['gx'],aa['gy']],axis=-1)*scale)
        gb=h(np.stack([bb['gx'],bb['gy']],axis=-1)*scale)
        dot=h(np.sum(h(ga*gb),axis=-1))
        k=h(np.clip(h(dot*(-.5)+.5),0,1)*h(f(smoothness)))
        numerator=h(h(da-db)*.5)
        if np.any((k==0)&(numerator==0)):
            raise ValueError('native NaN/0-over-0 case is not emulated')
        t=np.clip(h(h(numerator/k)+.5),0,1)
        distance=f(f(f(db-da)*t+da)+h(h(t*k)*h(t-1)))
        distance=h(h(distance)*127)
        gradient=h(h(h(gb-ga)*t[:,None]+ga)*127)
    if not np.isfinite(distance).all() or not np.isfinite(gradient).all():
        raise ValueError('half overflow is not emulated')
    result=np.empty(len(aa),PIXEL)
    result['distance']=np.clip(np.trunc(distance),-32768,32767).astype(np.int16)
    gradient=np.clip(np.trunc(gradient),-128,127).astype(np.int8)
    result['gx']=gradient[:,0];result['gy']=gradient[:,1]
    return result.tobytes()


def union_gpu(a, b, *, smoothness,backend_factory=None):
    """AIR float sdf_union on equally sized square float RGBA SDF fields.

    Channels: distance, gradient x/y, validity. Only b.w == 0 triggers the
    original empty-field replacement. The texel-fetch wrapper is our adapter.
    Reject undefined 0/0 and nonfinite arithmetic rather than invent a fallback.
    """
    import struct
    if __package__:
        from .render_validation import Renderer
    else:
        from render_validation import Renderer
    a=np.asarray(a,np.float32);b=np.asarray(b,np.float32)
    if a.ndim!=3 or a.shape!=b.shape or a.shape[2]!=4 or min(a.shape[:2])<1 or max(a.shape[:2])>4096:
        raise ValueError('equal finite RGBA SDF fields required')
    if not np.isfinite(a).all() or not np.isfinite(b).all() or not np.isfinite(smoothness) or not 0<=smoothness<=np.finfo(np.float32).max:
        raise ValueError('finite SDF inputs and nonnegative float32 smoothness required')
    effective=b.copy();effective[b[...,3]==0,:3]=(10000,0,0)
    with np.errstate(over='ignore',invalid='ignore'):
        k=np.clip(.5-.5*np.sum(a[...,1:3]*effective[...,1:3],axis=-1),0,1)*np.float32(smoothness)
        delta=effective[...,0]-a[...,0]
    if not np.isfinite(k).all() or not np.isfinite(delta).all() or np.any((k==0)&(delta==0)):
        raise ValueError('undefined or overflowing union arithmetic')
    with (backend_factory or Renderer)((a.shape[1],a.shape[0])) as renderer:
        result=renderer.render('sdf-union.frag',a,b,struct.pack('<4f',smoothness,0,0,0))
    if not np.isfinite(result).all():raise ValueError('nonfinite GPU union result')
    return result
