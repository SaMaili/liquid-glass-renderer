"""Pack rounded-rectangle distance-field geometry."""
import math
import struct
import numpy as np


def rectangle_geometry(size,radii,*,scale=1.,style_bits=0,radial=0.):
    if len(size)!=2 or any(not math.isfinite(v) or v<=0 for v in size):raise ValueError('positive finite size required')
    if not math.isfinite(scale) or not 0<scale<=1e6 or type(style_bits)!=int or not 0<=style_bits<=3:raise ValueError('positive scale and two style bits required')
    if not math.isfinite(radial):raise ValueError('finite radial modifier required')
    radii=(radii,)*4 if np.isscalar(radii) else tuple(radii)
    if len(radii)!=4 or any(not math.isfinite(v) or abs(v)>1e6 for v in radii):raise ValueError('four finite radii required')
    s=np.float32(scale);r=np.maximum(np.float32(.01),np.asarray(radii,np.float32))
    extent=(np.asarray(size,np.float64)*float(s)).astype(np.float32)*np.float32(.5)
    result=np.zeros((3,4),np.float32);result[0,:2]=extent;result[0,3]=radial;result[1]=(1,0,0,1)
    if np.all(r==r[0]):
        result[0,2]=4;radius=np.float32(r[0]*s)
        if style_bits:
            negative=struct.unpack('<d',bytes.fromhex('1bb6a4a7e821f7bf'))[0]
            positive=struct.unpack('<d',bytes.fromhex('1bb6a4a7e8210740'))[0]
            maximum=float(radius)*1.528665
            blend=np.clip((extent*np.float32(2)).astype(np.float64)*negative/maximum+positive,0,1).astype(np.float32)
        else:blend=(1,1)
        result[2]=(*blend,radius,0)
    else:
        result[0,2]=5;result[2]=r*s*(-1 if style_bits else 1)
    if not np.isfinite(result).all():raise ValueError('shape arithmetic overflow')
    return result
