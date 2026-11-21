"""Context/program reuse for an explicit host render graph, with bounded cache."""
from collections import OrderedDict
from contextlib import contextmanager
if __package__:
    from .render_validation import Renderer,dimensions
else:
    from render_validation import Renderer,dimensions
import threading


class RendererPool:
    def __init__(self,max_contexts=8):
        if type(max_contexts) is not int or max_contexts<1:raise ValueError('positive context limit required')
        self.limit=max_contexts;self.cache=OrderedDict();self.owner=threading.get_ident()
        self.closed=False;self.created=0;self.active=set()

    def _check(self):
        if self.closed:raise RuntimeError('pool is closed')
        if threading.get_ident()!=self.owner:raise RuntimeError('renderer pool belongs to its creating thread')

    @contextmanager
    def acquire(self,size):
        self._check();key=dimensions(size)
        if key in self.active:raise RuntimeError('context is already leased')
        if key not in self.cache:
            while len(self.cache)>=self.limit:
                candidates=[k for k in self.cache if k not in self.active]
                if not candidates:raise RuntimeError('all cached contexts are leased')
                self.cache.pop(candidates[0]).close()
            self.cache[key]=Renderer(key);self.created+=1
        self.cache.move_to_end(key);self.active.add(key)
        try:yield self.cache[key]
        finally:self.active.remove(key)

    def close(self):
        if self.closed:return
        self._check()
        if self.active:raise RuntimeError('cannot close leased contexts')
        for r in self.cache.values():r.close()
        self.cache.clear();self.closed=True

    def __enter__(self):self._check();return self
    def __exit__(self,*_):self.close()
