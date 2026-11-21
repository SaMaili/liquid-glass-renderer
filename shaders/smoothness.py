"""Bridge smoothness writes and return lifecycle, build 26A434.

The pulse/return springs use FluidAnimator. Host frame delivery is explicit;
the Apple UpdateGroup/transaction graph is not emulated by this adapter.
"""
from dataclasses import dataclass
import math,copy
if __package__:
    from .bridge import BridgeSettings
else:
    from bridge import BridgeSettings
if __package__:
    from .fluid import FluidAnimator, FluidSpring
else:
    from fluid import FluidAnimator, FluidSpring


@dataclass(frozen=True)
class SmoothnessOverride:
    value: float | None = None
    active: bool = False


def peak_value(base,settings=BridgeSettings()):
    # 0x2249a06c4: modifier +0x38 multiplied by settings +0x50.
    value=base*settings.smoothness_factor
    if not math.isfinite(value):raise ValueError('finite smoothness required')
    return value


class SmoothnessAnimator:
    def __init__(self,settings=BridgeSettings(),*,enabled=True):
        self.settings=settings
        self.enabled=enabled
        self.animator=None
        self.override=SmoothnessOverride()
        self.last_time=-math.inf
        self.listeners=[]

    def _deliver(self,time):
        pending=[];completed=False
        for listener in self.listeners:
            listener.sample(time)
            if listener.state.logically_complete or listener.finished:completed=True
            else:pending.append(listener)
        self.listeners=pending
        if completed:self.complete()

    def _time(self,time):
        if not math.isfinite(time) or time<self.last_time:raise ValueError('monotonic finite time required')
        self.last_time=time

    def pulse(self,base,*,time):
        self._time(time)
        self._deliver(time)
        if not self.enabled:return
        s=self.settings
        peak=peak_value(base,s)
        up=FluidSpring(s.smoothness_duration)
        down=FluidSpring(s.smoothness_return_duration,delay=s.smoothness_return_delay)
        if self.animator is None:
            self.animator=FluidAnimator(0.,peak,spring=up,time=time)
        else:
            self.animator.retarget(peak,time=time,spring=up)
        # Closures at 0x2249a0600 and 0x2249a0710: peak, then delayed zero.
        self.animator.retarget(0.,time=time,spring=down)
        self.override=SmoothnessOverride(self.animator.sample(time),True)
        # AnimatorState.forkListeners keeps the old registration's animation.
        # A later pulse does not cancel the previously registered completion.
        self.listeners.append(copy.deepcopy(self.animator))

    def sample(self,time):
        self._time(time)
        self._deliver(time)
        if self.animator is not None:
            value=self.animator.sample(time)
            self.override=SmoothnessOverride(value,True)
        return self.override

    def complete(self):
        # Completion at 0x2249a08a8 and 0x2249a0930: nil, then inactive.
        self.override=SmoothnessOverride()
        self.animator=None

    def cancel(self,*,time):
        self._time(time)
        # 0x22499ffd8: nil + active false. Host drops its local presentation.
        self.complete()
