"""Explicit host color contract; never guesses Apple's display profile.

Shader working values are linear-light premultiplied RGBA, no RGB clipping.
An optional caller-supplied 3x3 primary-conversion matrix operates on straight
linear RGB. Alpha stays linear and is required to lie in [0,1].
"""
import numpy as np


def rgba(image):
    a=np.asarray(image,dtype=np.float32)
    if a.ndim!=3 or a.shape[2]!=4 or min(a.shape[:2])<1 or not np.isfinite(a).all():
        raise ValueError('finite HxWx4 image required')
    if np.any((a[...,3]<0)|(a[...,3]>1)):
        raise ValueError('alpha must be in [0,1]')
    return a


def convert(image,*,source_transfer='linear',target_transfer='linear',
            source_alpha='premultiplied',target_alpha='premultiplied',matrix=None):
    a=rgba(image);alpha=a[...,3:4];rgb=a[...,:3].copy()
    if source_alpha not in ('straight','premultiplied') or target_alpha not in ('straight','premultiplied'):
        raise ValueError('explicit straight/premultiplied alpha required')
    if source_transfer not in ('linear','srgb') or target_transfer not in ('linear','srgb'):
        raise ValueError('supported transfers: linear or srgb')
    if source_alpha=='premultiplied':
        rgb=np.divide(rgb,alpha,out=np.zeros_like(rgb),where=alpha!=0)
    if source_transfer=='srgb':
        rgb=np.where(rgb<=.04045,rgb/12.92,np.power(np.maximum((rgb+.055)/1.055,0),2.4))
    if matrix is not None:
        m=np.asarray(matrix,np.float32)
        if m.shape!=(3,3) or not np.isfinite(m).all():raise ValueError('finite 3x3 color matrix required')
        rgb=rgb@m.T
    if target_transfer=='srgb':
        rgb=np.where(rgb<=.0031308,12.92*rgb,1.055*np.power(np.maximum(rgb,0),1/2.4)-.055)
    if target_alpha=='premultiplied':rgb*=alpha
    out=np.concatenate((rgb,alpha),axis=-1).astype(np.float32)
    if not np.isfinite(out).all():raise ValueError('color conversion overflow')
    return out


def over(front,back):
    front,back=rgba(front),rgba(back)
    if front.shape!=back.shape:raise ValueError('matching compositing dimensions required')
    return front+back*(1-front[...,3:4])
