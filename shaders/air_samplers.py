"""AIR0037 static sampler profile.

Only the ordinary 2D sampling profile used here is accepted, not comparison,
bicubic, pixel-coordinate, border-color or reduction samplers.
"""
import struct

NOISE_WORD = 34901797601018002
SOURCE_WORD = 34901797601036873
MASK_WORD = 34901797601028681


def decode_blur_sampler(word, second=0):
    if type(word) is not int or not 0 <= word < 2**64 or second != 0:
        raise ValueError("unsupported AIR sampler words")
    # These shaders share compare=never, anisotropy=1, LOD=[0,65504],
    # normalized coordinates, transparent border and weighted reduction.
    if word & ~0x7fff != 0x7bff0000080000:
        raise ValueError("sampler outside validated blur profile")
    address = tuple((word >> shift) & 7 for shift in (0, 3, 6))
    mag, minification, mip = ((word >> shift) & 3 for shift in (9, 11, 13))
    if any(a > 3 for a in address) or mag > 1 or minification > 1 or mip > 2:
        raise ValueError("unsupported sampler field")
    if len(set(address)) != 1:
        raise ValueError("renderer requires equal axis addressing")
    return {
        "wrap": ("zero", "clamp", "repeat", "mirror")[address[0]],
        "filters": (("nearest", "linear")[minification],
                    ("nearest", "linear")[mag], ("none", "nearest", "linear")[mip]),
        "lod_min": struct.unpack("<e", ((word >> 24) & 65535).to_bytes(2, "little"))[0],
        "lod_max": struct.unpack("<e", ((word >> 40) & 65535).to_bytes(2, "little"))[0],
    }


BLUR_SAMPLERS = tuple(decode_blur_sampler(w) for w in (SOURCE_WORD, MASK_WORD, NOISE_WORD))
BLUR_WRAPS = tuple(s["wrap"] for s in BLUR_SAMPLERS)
BLUR_FILTERS = tuple(s["filters"] for s in BLUR_SAMPLERS)

DOWNSAMPLE_SAMPLER = decode_blur_sampler(34901797601028608)

# Material AIR0057..0074 and 0197..0214 use the same source/SDF words.
# The third slot is unused by material passes. See material-samplers.json.
MATERIAL_WRAPS = (BLUR_WRAPS[0], BLUR_WRAPS[1], "clamp")
MATERIAL_FILTERS = (BLUR_FILTERS[0], BLUR_FILTERS[1], ("linear","linear","none"))
