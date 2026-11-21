"""26A434 bridge defaults and CGRect bridgingTransform for finite rects.

A default zero-velocity timeline is provided, not the SwiftUI transaction engine.
No claim about interruption, overrides or full contour pairing.
"""
import math
from dataclasses import dataclass

@dataclass(frozen=True)
class BridgeSettings:
    major_duration: float = .5
    major_delay: float = 0.
    minor_duration: float = .3
    minor_delay: float = .1
    smoothness_factor: float = 1.
    smoothness_duration: float = .5
    smoothness_return_delay: float = .1
    smoothness_return_duration: float = .3

def critical_decay(time, duration, delay=0.):
    """Bounce=0, start 1, end 0, zero velocity; Spring.value critical branch."""
    if not all(math.isfinite(x) for x in (time,duration,delay)) or duration<=0:
        raise ValueError('finite time/delay and positive duration required')
    t=max(0.,time-delay);x=2*math.pi/duration*t
    return 0. if x>745 else (1+x)*math.exp(-x)

def progress(time, settings=BridgeSettings()):
    # animateProgress substitutes minorDownDelay into the major Spring delay;
    # animateMinor receives max(majorDownDelay,0). Preserve that observed wiring.
    return (critical_decay(time,settings.major_duration,settings.minor_delay),
            critical_decay(time,settings.minor_duration,max(settings.major_delay,0.)))

def transform(other, rect, major, minor):
    """Reconstructed bridgingTransform: transform rect toward other, returning (a,b,c,d,tx,ty).

    CoreGraphics standardizes extents for getters and each actual outset.
    Zero source dimensions keep scale 1, as in the original function.
    """
    source=other;target=rect
    if len(source)!=4 or len(target)!=4 or not all(math.isfinite(v) for v in (*source,*target,major,minor)):
        raise ValueError('finite rectangles and progress required')
    ax,ay,aw,ah=source;bx,by,bw,bh=target
    ac=(ax+min(aw,0)+abs(aw)/2,ay+min(ah,0)+abs(ah)/2)
    bc=(bx+min(bw,0)+abs(bw)/2,by+min(bh,0)+abs(bh)/2)
    lo=[min(ac[i],bc[i]) for i in range(2)];hi=[max(ac[i],bc[i]) for i in range(2)]
    vertical=(hi[1]-lo[1]) >= (hi[0]-lo[0])
    w=3+minor*(bw-3) if vertical else bw-6
    h=bh-6 if vertical else 3+minor*(bh-3)
    x=bx+abs(bw)/2-w/2;y=by+abs(bh)/2-h/2
    axis=1 if vertical else 0
    start=y+min(h,0) if vertical else x+min(w,0)
    extension=max(0,major*(start-lo[axis]))
    if extension>0:
        x+=min(w,0);y+=min(h,0);w=abs(w);h=abs(h)
        if vertical:y-=extension;h+=extension
        else:x-=extension;w+=extension
    end=y+min(h,0)+abs(h) if vertical else x+min(w,0)+abs(w)
    extension=max(0,major*(hi[axis]-end))
    if extension>0:
        x+=min(w,0);y+=min(h,0);w=abs(w);h=abs(h)
        if vertical:h+=extension
        else:w+=extension
    sx=abs(w)/abs(bw) if bw else 1.;sy=abs(h)/abs(bh) if bh else 1.
    result=(sx,0.,0.,sy,x-bx*sx,y-by*sy)
    if not all(math.isfinite(v) for v in result):raise ValueError('bridge arithmetic overflow')
    return result

def frame(other,rect,time,settings=BridgeSettings()):
    major,minor=progress(time,settings)
    return {'major':major,'minor':minor,'transform':transform(other,rect,major,minor)}


def critical_state(time, *, start, target, velocity, duration):
    """Critical Spring solution with explicit initial velocity; returns value/velocity.

    This exposes the recovered spring equation. Choice of velocity on retarget is
    a host policy, not a claim that all SwiftUI transactions preserve velocity.
    """
    if not all(math.isfinite(x) for x in (time,start,target,velocity,duration)) or time<0 or duration<=0:
        raise ValueError('finite state, nonnegative elapsed time and positive duration required')
    omega=2*math.pi/duration
    if not math.isfinite(omega):raise ValueError('duration too small')
    if time==0:return (start,velocity)
    x=omega*time
    if x>745:return (target,0.)
    delta=start-target;coefficient=velocity+omega*delta
    decay=math.exp(-x)
    value=target+(delta+coefficient*time)*decay
    speed=(velocity-omega*coefficient*time)*decay
    if not math.isfinite(value) or not math.isfinite(speed):raise ValueError('spring arithmetic overflow')
    return value,speed

@dataclass(frozen=True)
class CriticalTransition:
    start: float
    target: float
    velocity: float
    duration: float
    started_at: float

    def sample(self,time):
        if not math.isfinite(time) or not math.isfinite(self.started_at) or time<self.started_at:
            raise ValueError('sample time precedes transition')
        return critical_state(time-self.started_at,start=self.start,target=self.target,velocity=self.velocity,duration=self.duration)

    def retarget(self,time,target,*,duration,carry_velocity):
        """Retarget continuously; caller explicitly chooses velocity policy."""
        if type(carry_velocity)!=bool:raise ValueError('explicit boolean velocity policy required')
        value,velocity=self.sample(time)
        next_transition=CriticalTransition(value,target,velocity if carry_velocity else 0.,duration,time)
        next_transition.sample(time)
        return next_transition
