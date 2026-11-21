"""Scalar/Euclidean-vector FluidSpringAnimation state primitive recovered from build 26A434.

Values are in the caller's animation coordinate system. This is not SwiftUI's
attribute graph or transaction dispatch. See BLUR-SAMPLER-UND-FLUID.md.
"""
import copy
import math
import struct
from dataclasses import dataclass
import numpy as np


def _value(value):
    a = np.asarray(value, dtype=np.float64)
    if a.ndim > 1 or a.size == 0 or not np.isfinite(a).all():
        raise ValueError("finite scalar or nonempty flat vector required")
    return float(a) if a.ndim == 0 else a.copy()


def _norm_squared(value):
    a = np.asarray(value, dtype=np.float64)
    return float(np.sum(a*a))


def _double(bits):
    return struct.unpack("<d", bits.to_bytes(8, "little"))[0]


STEP = _double(0x3f6b4e81b4e81b4f)
HALF_STEP = _double(0x3f5b4e81b4e81b4f)
MAX_STIFFNESS = _double(0x40e5f8ffffffffff)
LONG_GAP = _double(0x3f91111111111111)


@dataclass(frozen=True)
class FluidSpring:
    response: float = .5
    damping: float = 1.
    blend_duration: float = 0.
    delay: float = 0.

    def __post_init__(self):
        if not all(math.isfinite(x) for x in (
                self.response, self.damping, self.blend_duration, self.delay)):
            raise ValueError("finite fluid spring parameters required")
        if self.response <= 0 or min(self.damping, self.blend_duration, self.delay) < 0:
            raise ValueError("positive response and nonnegative other parameters required")


@dataclass
class FluidState:
    offset: float = 0.
    velocity: float = 0.
    force: float = 0.
    time: float = 0.
    start_time: float = 0.
    blend_start: float = 0.
    blend_interval: float = 0.
    active_delay: object = None
    logically_complete: bool = False


@dataclass
class Delayed:
    previous_value: float
    previous_spring: FluidSpring
    state: FluidState


def consume_delay(state, delayed):
    """0x224eace00: retain an earlier transition; supersede equal/later ones."""
    existing = state.active_delay
    if existing is not None and existing.state.time < delayed.state.time:
        consume_delay(existing.state, delayed)
    else:
        state.active_delay = copy.deepcopy(delayed)


def response_at(state,time,spring):
    response = spring.response
    if spring.blend_duration > 0 and state.blend_interval != 0:
        t = max(0., min(1., (time - (spring.delay + state.blend_start)) / spring.blend_duration))
        response += state.blend_interval * (1. - t*t*(3. - 2.*t))
    return response


def integrate(state, value, time, spring):
    """Numeric block 0x224eaa450..0x224eaa5c4, before finish predicates."""
    response=response_at(state,time,spring)
    stiffness = min((2.*math.pi/response)**2 if response > 0 else math.inf, MAX_STIFFNESS)
    if time - state.time > 1.:
        state.time = time - LONG_GAP
    damping = -spring.damping * (2.*math.sqrt(stiffness))
    while state.time < time:
        state.velocity += state.force * HALF_STEP
        state.offset += state.velocity * STEP
        state.force = damping * state.velocity + stiffness * (value - state.offset)
        state.velocity += state.force * HALF_STEP
        state.time += STEP


def advance(state, value, time, spring):
    """Advance in-place. Return offset, or None at the default completion gate.

    Custom AnimationFinishingDefinition environment overrides are not modeled.
    Exact delay equality intentionally retains the previous animation.
    """
    value = _value(value)
    if not all(math.isfinite(x) for x in (time, state.time)):
        raise ValueError("finite time required")
    while state.active_delay is not None and state.active_delay.state.time < time:
        next_state = copy.deepcopy(state.active_delay.state)
        next_state.offset, next_state.velocity = state.offset, state.velocity
        state.__dict__.update(next_state.__dict__)
    selected = state.active_delay
    if selected is not None:
        value, spring = selected.previous_value, selected.previous_spring
    if spring.delay > 0 and time < spring.delay + state.start_time:
        state.time = time
        return state.offset
    if state.active_delay is None and response_at(state,time,spring)<=time-(spring.delay+state.start_time):
        state.logically_complete=True
    integrate(state, value, time, spring)
    if state.active_delay is not None:
        return state.offset
    # Default scalar completion branch, 0x224eaa7c8..0x224eaa820.
    if max(_norm_squared(state.velocity), _norm_squared(state.force)) > _double(0x3f6d7dbf487fcb92):
        return state.offset
    tolerance = _norm_squared(value * .01)
    if tolerance > 0 and tolerance < _norm_squared(value-state.offset):
        return state.offset
    return None


def merge(state, *, previous_spring, new_spring, previous_value, time,
          previous_velocity=None, previous_output=None):
    """Recovered shouldMerge state writes; returns True, no carry policy flag.

    Fluid previous: velocity comes from state, animate is invoked on the old
    state when no delay is scheduled. A non-fluid previous requires the output
    and optional velocity from its own animation implementation. Missing output
    falls back to previous_value; missing velocity preserves the stored one.
    """
    previous_value = _value(previous_value)
    if not math.isfinite(time):
        raise ValueError("finite merge time required")
    next_state = copy.deepcopy(state)
    next_state.start_time = time
    next_state.active_delay = None
    next_state.logically_complete = False
    if previous_spring is not None:
        if previous_spring.response != new_spring.response:
            next_state.blend_interval = previous_spring.response-new_spring.response
            next_state.blend_start = time
        if new_spring.delay > 0:
            next_state.time = time+new_spring.delay
            consume_delay(state, Delayed(previous_value, previous_spring, next_state))
            return True
        # The original asks velocity BEFORE calling previous.animate.
        previous_velocity = copy.deepcopy(state.velocity)
        previous_output = advance(state, previous_value, time, previous_spring)
    next_state.time = time
    if previous_velocity is not None:
        next_state.velocity = previous_velocity
    next_state.offset = previous_value if previous_output is None else previous_output
    state.__dict__.update(next_state.__dict__)
    return True


class FluidAnimator:
    """Automatic scalar/vector target changes using AnimatorState's merged interval.

    combine adds the new target delta to the existing interval (0x224ed86e0);
    update emits model + animated_offset - interval (0x224eb4b54..b64).
    Explicit frame times; no SwiftUI listener, frame throttling or environment.
    """
    def __init__(self, start, target, *, spring=FluidSpring(), time=0.):
        start,target=_value(start),_value(target)
        if np.shape(start)!=np.shape(target) or not math.isfinite(time):
            raise ValueError("matching value shapes and finite initial time required")
        self.target=target
        self.interval=target-start
        self.spring=spring
        self.origin=time
        self.last_time=time
        self.state=FluidState()
        self.finished=False

    def _time(self,time):
        if not math.isfinite(time) or time<self.last_time:
            raise ValueError("monotonic finite frame times required")
        return time-self.origin

    def sample(self,time):
        elapsed=self._time(time)
        self.last_time=time
        if self.finished:return copy.deepcopy(self.target)
        value=advance(self.state,self.interval,elapsed,self.spring)
        if value is None:
            self.finished=True
            return copy.deepcopy(self.target)
        return self.target+value-self.interval

    def retarget(self,target,*,time,spring=None):
        elapsed=self._time(time)
        target=_value(target)
        if np.shape(target)!=np.shape(self.target):raise ValueError("target shape changed")
        selected=self.spring if spring is None else spring
        if self.finished:
            old=self.target
            self.__init__(old,target,spring=selected,time=time)
            return
        merge(self.state,previous_spring=self.spring,new_spring=selected,
              previous_value=self.interval,time=elapsed)
        self.interval+=target-self.target
        self.target=target
        self.spring=selected
        self.last_time=time
