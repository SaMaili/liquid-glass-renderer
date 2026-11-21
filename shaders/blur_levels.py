"""Scalar portion of compute_variable_blur_parameters, build 26A434.

Arguments retain ABI names until caller semantics are established. Includes
finite/empty/infinite bounds; the native kernel/copy schedule remains separate.
Host log2f is used; Apple libm boundary equivalence is not established.
"""
import ctypes,ctypes.util,math,struct
_libm=ctypes.CDLL(ctypes.util.find_library('m'))
_libm.log2f.argtypes=[ctypes.c_float];_libm.log2f.restype=ctypes.c_float

def f32(x):return struct.unpack('<f',struct.pack('<f',x))[0]
def scalar_levels(width,height,*,s0,s1,s2,flag):
    if any(type(v)!=int or not 1<=v<=0xffffffff for v in (width,height)):
        raise ValueError('positive uint32 dimensions required')
    if type(flag)!=bool:raise ValueError('boolean flag required')
    try:s0,s1,s2=map(f32,(s0,s1,s2))
    except (OverflowError,struct.error):raise ValueError('float32 scalars required') from None
    if not all(math.isfinite(x) for x in (s0,s1,s2)) or min(s0,s1)<0 or s2<=0:
        raise ValueError('s0/s1 nonnegative; s2 positive and finite required')
    low=f32(s0*1.6);high=f32(s1*1.6)
    if not math.isfinite(low) or not math.isfinite(high):raise ValueError('scaled float overflow')
    capacity=math.floor(_libm.log2f(f32(max(width,height))))+1
    requested=1 if high==0 else max(0,math.ceil(_libm.log2f(high)))+1
    if requested==1 and high!=0:requested=2
    count=min(requested,capacity)
    capped=min(count,6 if flag else 7)
    exponent=max(0,math.ceil(_libm.log2f(s2))+capped)
    if exponent>30:raise ValueError('signed shift/wrap region not supported')
    first=0 if low==0 else max(0,math.floor(_libm.log2f(low)))
    first=min(first,max(0,count-1))
    return {'scaled_s0':low,'scaled_s1':high,'first_level':first,
            'level_count':count,'capped_count':capped,
            'stable_exponent':exponent,'alignment_scale':float(1<<exponent)}

INFINITE_BOUNDS=(-536870911,-536870911,1073741823,1073741823)
MAX_DOUBLE=float.fromhex('0x1.fffffffffffffp+1023')


def parameters(width,height,*,bounds,s0,s1,s2,flag):
    """Finite, empty/negative-size and original unbounded Bounds branches.

    None requests the native infinite Bounds sentinel. No huge GPU allocation
    is implied: a host must intersect unbounded geometry with a finite target.
    """
    if bounds is None:bounds=INFINITE_BOUNDS
    if len(bounds)!=4 or any(type(v)!=int or not -2**31<=v<2**31 for v in bounds):
        raise ValueError('signed int32 Bounds required')
    result=scalar_levels(width,height,s0=s0,s1=s1,s2=s2,flag=flag)
    if max(bounds[2:])>0x3ffffffe:
        result['bounds']=INFINITE_BOUNDS
        result['rect']=(-MAX_DOUBLE/2,-MAX_DOUBLE/2,MAX_DOUBLE,MAX_DOUBLE)
        result['unbounded']=True
        return result
    x,y,w,h=map(float,bounds);radius=f32(s1)
    if min(w,h)>0:
        x-=radius*2.8;y-=radius*2.8;w+=radius*5.6;h+=radius*5.6
        scale=result['alignment_scale']
        x0=math.floor(x/scale)*scale;y0=math.floor(y/scale)*scale
        x1=math.ceil((x+w)/scale)*scale;y1=math.ceil((y+h)/scale)*scale
        x,y,w,h=float(x0),float(y0),float(x1-x0),float(y1-y0)
    result['rect']=(x,y,w,h)
    if max(w,h)>=1073741823.:
        result['bounds']=INFINITE_BOUNDS;result['unbounded']=True
    else:
        wrap=lambda v:(int(v)+2**31)%2**32-2**31
        result['bounds']=tuple(map(wrap,(x,y,w,h)));result['unbounded']=False
    return result
