"""Named lpf/lph buffer contracts. Requires every value; no material defaults."""
import json,math,struct
from pathlib import Path
import numpy as np
from collections.abc import Mapping

LAYOUT=json.loads((Path(__file__).resolve().parent/'uniform-layout.json').read_text())
SIZES={'foreground':80,'background':320,'background_ext':28,'foreground_lph':80,'background_lph':240}
NATIVE_INFINITE_RECIPROCALS=frozenset(('inner_refraction_inv_height','outer_refraction_inv_height',
    'edge_bleed_inv_height','shadow_inv_height','shadow_inv_radius','key_fill_highlight_amount',
    'inv_aberration_height'))


def pack(values,kind):
    if kind not in SIZES:raise ValueError('unknown uniform kind')
    if not isinstance(values,Mapping):raise ValueError('named uniform mapping required')
    fields=LAYOUT[kind]
    names={f['name'] for f in fields}
    if set(values)!=names:raise ValueError(f'missing={sorted(names-set(values))}, unknown={sorted(set(values)-names)}')
    result=bytearray(SIZES[kind])
    for f in fields:
        width=2 if f['type'].startswith('half') else 1 if f['type']=='bool' else 4
        code={1:'B',2:'e',4:'f'}[width]
        a=np.asarray(values[f['name']]).reshape(-1)
        numeric=a.astype(float)
        allowed=np.logical_not(np.isnan(numeric)) if f['name'] in NATIVE_INFINITE_RECIPROCALS else np.isfinite(numeric)
        if len(a)!=f['size']//width or not allowed.all():raise ValueError(f"invalid {f['name']}")
        if width==1 and any(v not in (0,1) for v in a):raise ValueError('boolean field required')
        try:struct.pack_into('<'+str(len(a))+code,result,f['offset'],*(int(v) if width==1 else float(v) for v in a))
        except (OverflowError,struct.error) as exc:raise ValueError(f"out of range {f['name']}") from exc
    return bytes(result)


def unpack(payload,kind):
    if kind not in SIZES or len(payload)!=SIZES[kind]:raise ValueError('invalid uniform payload')
    result={}
    for f in LAYOUT[kind]:
        width=2 if f['type'].startswith('half') else 1 if f['type']=='bool' else 4
        values=struct.unpack_from('<'+str(f['size']//width)+{1:'B',2:'e',4:'f'}[width],payload,f['offset'])
        result[f['name']]=values[0] if len(values)==1 else values
    return result
