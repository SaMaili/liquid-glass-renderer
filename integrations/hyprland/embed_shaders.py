"""Embed the same recovered kernels used by the Rust reference."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2] / "shaders"
preamble = "#version 320 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;"
parts = ["#pragma once\n"]
for name, filename in (("distanceShader", "sdf-probe"), ("blurShader", "blur-agx2"),
                       ("materialShader", "background-all")):
    shader = (root / f"{filename}.frag").read_text().replace("#version 450", preamble)
    if name == "distanceShader":
        shader = shader.replace("vec4 data[3]", "vec4 data[6]")
        shader = shader.replace("layout(location=0) in vec2 sdfUV;",
                                "layout(location=0) in vec2 sdfUV;\nlayout(location=1) in vec2 tipUV;")
        shader = shader.replace("vec4 computeSdf(vec2 p, int mode) {",
            "vec4 computeSdf(vec2 p, int offset) {\n    int mode=int(abs(geometry(offset).z));")
        shader = shader.replace("geometry(0), m=geometry(1), b=geometry(2)",
                                "geometry(offset), m=geometry(offset+1), b=geometry(offset+2)")
        union_source = (root / "sdf-union.frag").read_text()
        union = union_source[union_source.index("vec4 sdfUnion"):union_source.index("void main")]
        union = union.replace("    float h=", "    if (k == 0.0) return a.x < b.x ? a : b;\n    float h=")
        shader = shader[:shader.index("void main")] + union + """
uniform bool cartouche;
uniform int mitosisCount;
uniform float mitosisSmoothness;
uniform float mitosisInvScale;
uniform vec4 mitosisCircles[4];

vec4 ellipseSdf(vec2 pt, vec4 e) {
    vec2 r = max(e.zw, vec2(0.001));
    vec2 p = pt - e.xy;
    vec2 q = p / r;
    float k0 = length(q);
    float k1 = length(p / (r * r));
    float dist = k0 > 0.0001 ? k0 * (k0 - 1.0) / max(k1, 0.0001) : -min(r.x, r.y);
    vec2 grad = k1 > 0.0001 ? (p / (r * r)) / k1 : vec2(1.0, 0.0);
    return vec4(dist, grad.x, grad.y, 1.0);
}

void main() {
    vec4 body = computeSdf(sdfUV, 0);
    if (cartouche)
        body = sdfUnion(body, computeSdf(tipUV, 3), 25.0);
    if (mitosisCount > 0) {
        vec2 pt = gl_FragCoord.xy * mitosisInvScale;
        for (int i = 0; i < 4; ++i) {
            if (i >= mitosisCount) break;
            body = sdfUnion(body, ellipseSdf(pt, mitosisCircles[i]), mitosisSmoothness);
        }
    }
    outputColor = body;
}
"""
    if name == "blurShader":
        shader = shader.replace("textureLod(sourceTexture,uv+d*u.p.xy,0)", "textureLod(sourceTexture,uv+d*u.p.xy,0.0)")
        shader = shader.replace("uniform Params {vec4 p;} u;", "uniform Params {vec4 p;} u;\nuniform float sourceLevel;")
        shader = shader.replace("u.p.xy,0.0)", "u.p.xy,sourceLevel)")
    if name == "materialShader":
        shader = shader.replace("uint word(int byteOffset)", """
uniform float legibilityLod;
uniform bool dockSrgbFace;
uniform vec4 widgetPlate[3];
uniform vec4 widgetTint;
uniform vec4 widgetTint2;
uniform vec4 widgetTintAxis;

vec3 encodeWidget(vec3 linearRGB) {
    return mix(1.055 * pow(max(linearRGB, 0.0), vec3(1.0 / 2.4)) - 0.055,
               12.92 * linearRGB, lessThanEqual(linearRGB, vec3(0.0031308)));
}
vec3 decodeWidget(vec3 srgb) {
    return mix(pow((srgb+0.055)/1.055, vec3(2.4)), srgb/12.92,
               lessThanEqual(srgb, vec3(0.04045)));
}
vec3 measuredWidgetPlate(vec3 linearRGB) {
    vec3 srgb = clamp(encodeWidget(linearRGB), 0.0, 1.0);
    float luma = dot(srgb, vec3(0.2126, 0.7152, 0.0722));
    vec3 plate = vec3(dot(widgetPlate[0].xyz, vec3(srgb.r,luma,1)),
                      dot(widgetPlate[1].xyz, vec3(srgb.g,luma,1)),
                      dot(widgetPlate[2].xyz, vec3(srgb.b,luma,1)));
    return mix(decodeWidget(clamp(plate, 0.0, 1.0)), decodeWidget(widgetTint.rgb), widgetTint.a);
}
// Returns 0..1 in the transparent bottom half of a vertical Dual-Tint widget
// (widgetTint.a > 0, widgetTint2.a == 0) so the lower half stays 100% crystal-clear
// un-darkened refracting Liquid Glass even over bright white backgrounds.
float clearBottomFactor() {
    if (legibilityLod >= 0.0 || widgetTint.a <= 0.0 || widgetTint2.a > 0.001) return 0.0;
    float vNorm = clamp((gl_FragCoord.y - widgetTintAxis.x) / max(widgetTintAxis.y, 1.0), 0.0, 1.0);
    return smoothstep(widgetTintAxis.z, max(widgetTintAxis.w, widgetTintAxis.z + 0.001), vNorm);
}
uint word(int byteOffset)""")
        shader = shader.replace("float lodForDistance(float distance) {", """float lodForDistance(float distance) {
    if (legibilityLod >= 0.0) return legibilityLod;""")
        shader = shader.replace("vec3 mappedFace(vec3 rgb) {", """vec3 srgbFaceMatrix(vec3 linearRGB) {
    vec3 adjusted = clamp(encodeWidget(linearRGB), 0.0, 1.0);
    if(E(16)>0.0) {
        float luma=dot(vec3(0.2125999927520752,0.7152000069618225,0.0722000002861023),adjusted);
        float c=sat(1.0-luma*E(16));
        adjusted=mix(vec3(c*luma),c*adjusted,vec3(1.0+(1.0-c)*0.30000001192092896));
    }
    vec4 r=H4(80),g=H4(88),b=H4(96);
    vec3 mapped=vec3(dot(adjusted,r.rgb)+r.a,dot(adjusted,g.rgb)+g.a,dot(adjusted,b.rgb)+b.a);
    return decodeWidget(clamp(mapped, 0.0, 1.0));
}
vec3 mappedFace(vec3 rgb) {
    if (legibilityLod >= 0.0) return measuredWidgetPlate(rgb);
    if (dockSrgbFace) return srgbFaceMatrix(rgb);""")
        shader = shader.replace("if(flag(21))", "if(flag(21) && legibilityLod < 0.0)")
        shader = shader.replace("        if(U(204)>0.0) {",
            "        if(legibilityLod >= 0.0) face.rgb = measuredWidgetPlate(rgb);\n        else if(dockSrgbFace && U(204)>0.0) face = vec4(mix(rgb, srgbFaceMatrix(rgb), U(204)), face.a);\n        else if(U(204)>0.0) {", 1)
        shader = shader.replace("face=vec4(mix(rgb,mapped,U(204)),face.a);",
            "face=vec4(mix(rgb,mapped,U(204)*(1.0-clearBottomFactor())),face.a);", 1)
        shader = shader.replace("face.rgb=mix(face.rgb,bleed,weight);",
            "face.rgb=mix(face.rgb,bleed,weight*(1.0-clearBottomFactor()));", 1)
        shader = shader.replace("bleed=vec3(dot(bleed,br.rgb)", "bleed=legibilityLod >= 0.0 ? measuredWidgetPlate(bleed) : vec3(dot(bleed,br.rgb)")
        # The SDF is a full-resolution raster. Read the other lanes in the
        # current fragment quad explicitly to reproduce the fine derivative.
        shader = shader.replace("fwidthFine(sdf.r)", """(
        abs(sdf.r - texelFetch(sdfTexture, min(p ^ ivec2(1, 0), sdfMax), 0).r) +
        abs(sdf.r - texelFetch(sdfTexture, min(p ^ ivec2(0, 1), sdfMax), 0).r))""")
        shader = shader.replace("    float width=max", """    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 sdfMax = textureSize(sdfTexture, 0) - ivec2(1);
    float width=max""")
        shader = shader.replace("void main()", "void materialLayer()")
        shader += """
uniform float widgetOpacity;
uniform vec4 highlightMatrix[4];
uniform vec4 highlightLobes[2];
layout(binding=5) uniform sampler2D surfaceMask;
uniform float surfacePedestal;
uniform ivec2 maskOffset;

// Exact 26A434 UberShader::sdf_diffuse_kf_highlight (module-0232.ll, opcode 0x44)
// + UberShader::backdrop_aware_vibrant_color_matrix_sover.
vec3 applyKeyFillHighlight(vec3 dst) {
    float hlOpacity = highlightMatrix[3].w * widgetOpacity;
    if (hlOpacity <= 0.0) return dst;
    vec4 sdf = texture(sdfTexture, sdfUV);
    if (sdf.a <= 0.0) return dst;
    float dIn = -(sdf.r + highlightLobes[1].w);
    if (dIn < -5.0) return dst;
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 sdfMax = textureSize(sdfTexture, 0) - ivec2(1);
    float w = max(
        abs(sdf.r - texelFetch(sdfTexture, min(p ^ ivec2(1, 0), sdfMax), 0).r) +
        abs(sdf.r - texelFetch(sdfTexture, min(p ^ ivec2(0, 1), sdfMax), 0).r),
        0.0001);
    float gradLen = sqrt(dot(sdf.gb, sdf.gb));
    vec2 n = sdf.gb / max(gradLen, 0.000001);
    float dotK = dot(highlightMatrix[3].xy, n);
    float dotF = -dotK;

    // Sharp key & fill lobes (with curvature mix).
    float h = highlightLobes[0].x;
    float s = highlightLobes[0].y;
    float amt = highlightLobes[0].z;
    float curv = highlightLobes[0].w;
    float t = sat(dIn / h);
    float prof = mix(t < 1.0 ? 1.0 : 0.0, 1.0 - t, curv);
    float edge = sat(dIn / w + 0.5) * sat((h - dIn) / w + 0.5) * prof;
    float invSpread = 1.0 / max(1.0 - s, 0.000001);
    float kRaw = edge * sat((dotK - s) * invSpread);
    float fRaw = edge * sat((dotF - s) * invSpread);

    // Diffuse key & fill lobes (linear 1 - sat(dIn / hD) profile).
    float hD = highlightLobes[1].x;
    float sD = highlightLobes[1].y;
    float amtD = highlightLobes[1].z;
    float edgeD = sat(dIn / w + 0.5) * sat((hD - dIn) / w + 0.5) * (1.0 - sat(dIn / hD));
    float invSpreadD = 1.0 / max(1.0 - sD, 0.000001);
    float kdRaw = edgeD * sat((dotK - sD) * invSpreadD);
    float fdRaw = edgeD * sat((dotF - sD) * invSpreadD);

    if (kRaw + fRaw + kdRaw + fdRaw < 0.000001) return dst;

    float kSharp = kRaw / max(1.0 + amt * (1.0 - kRaw), 0.000001);
    float fSharp = fRaw / max(1.0 + amt * (1.0 - fRaw), 0.000001);
    float kDiff = kdRaw / max(1.0 + amtD * (1.0 - kdRaw), 0.000001);
    float fDiff = fdRaw / max(1.0 + amtD * (1.0 - fdRaw), 0.000001);
    float alpha = sat((kSharp + kDiff + fSharp + fDiff) * sdf.a) * hlOpacity;
    if (alpha <= 0.0) return dst;

    // backdrop_aware_vibrant_color_matrix_sover over dst (backdrop.a == 1.0).
    vec3 mapped = vec3(
        dot(dst, highlightMatrix[0].xyz) + highlightMatrix[0].w,
        dot(dst, highlightMatrix[1].xyz) + highlightMatrix[1].w,
        dot(dst, highlightMatrix[2].xyz) + highlightMatrix[2].w);
    float clampMax = highlightMatrix[3].z;
    if (clampMax > 0.0) mapped = clamp(mapped, vec3(-0.75), vec3(clampMax));
    return mapped * alpha + dst * (1.0 - alpha);
}

float dualTintCubic(float t, float p1, float p2) {
    float s = 1.0 - t;
    return 3.0 * s * t * (s * p1 + t * p2) + t * t * t;
}

float dualTintFade(float position) {
    if (position <= 0.0) return 1.0;
    if (position >= 1.0) return 0.0;
    float lo = 0.0, hi = 1.0;
    for (int i = 0; i < 16; ++i) {
        float mid = (lo + hi) * 0.5;
        if (dualTintCubic(mid, 0.60938, 0.47124) < position) lo = mid;
        else hi = mid;
    }
    return 1.0 - dualTintCubic((lo + hi) * 0.5, 0.00663, 0.99115);
}

vec3 applyDualTint(vec3 linearRGB) {
    if (legibilityLod >= 0.0 || (widgetTint.a <= 0.0 && widgetTint2.a <= 0.0)) return linearRGB;
    ivec2 p = ivec2(gl_FragCoord.xy);
    ivec2 sdfMax = textureSize(sdfTexture, 0) - 1;
    vec4 sdf = texelFetch(sdfTexture, clamp(p, ivec2(0), sdfMax), 0);
    float w = clamp(fwidth(sdf.r), 0.0001, 1.0);
    float groupCov = clamp(0.5 - sdf.r / max(w, 0.0001), 0.0, 1.0) * clamp(sdf.a, 0.0, 1.0);
    if (groupCov <= 0.0) return linearRGB;

    // Native Dual-Tint / Vertical Gradient Tint:
    // - Interpolates smoothly between widgetTint (top) and widgetTint2 (bottom) along widgetTintAxis
    //   (e.g. Siri: dark tinted upper half -> 100% transparent crystal-clear lower half).
    // - Premultiplied linear interpolation avoids dark/gray fringes when fading to alpha = 0.
    // - Inward cubic-Bezier SDF ramp keeps the outer refractive meniscus rim luminous & clear.
    float vNorm = clamp((gl_FragCoord.y - widgetTintAxis.x) / max(widgetTintAxis.y, 1.0), 0.0, 1.0);
    float gradT = smoothstep(widgetTintAxis.z, max(widgetTintAxis.w, widgetTintAxis.z + 0.001), vNorm);
    vec3 premulTop = decodeWidget(widgetTint.rgb) * widgetTint.a;
    vec3 premulBot = decodeWidget(widgetTint2.rgb) * widgetTint2.a;
    vec3 premulRgb = mix(premulTop, premulBot, gradT);
    float tintAlpha = mix(widgetTint.a, widgetTint2.a, gradT);
    if (tintAlpha <= 0.0001) return linearRGB;

    float inwardDepth = max(0.0, -sdf.r);
    float innerWeight = 1.0 - dualTintFade(clamp(inwardDepth / 10.0, 0.0, 1.0));
    float rimToCore = mix(0.55, 1.0, innerWeight);
    float cov = groupCov * rimToCore * widgetOpacity;
    return premulRgb * cov + linearRGB * (1.0 - tintAlpha * cov);
}

void main() {
    vec4 surf = vec4(0.0);
    if (surfacePedestal > 0.0) {
        surf = texelFetch(surfaceMask, maskOffset + ivec2(gl_FragCoord.xy), 0);
        float d = texelFetch(sdfTexture, ivec2(gl_FragCoord.xy), 0).r;
        if (d > 24.0 && surf.a < 0.004) {
            vec3 bg = texelFetch(sourceTexture, ivec2(gl_FragCoord.xy), 0).rgb;
            outputColor = vec4(encodeWidget(bg), 1.0);
            return;
        }
    }
    materialLayer();
    vec4 layer = outputColor * widgetOpacity;
    vec3 backdrop = texelFetch(sourceTexture, ivec2(gl_FragCoord.xy), 0).rgb;
    vec3 baseLinear = layer.rgb + backdrop * (1.0 - layer.a);
    vec3 litSrgb = applyKeyFillHighlight(encodeWidget(baseLinear));
    vec3 srgb = (legibilityLod < 0.0 && (widgetTint.a > 0.0 || widgetTint2.a > 0.0))
        ? encodeWidget(applyDualTint(decodeWidget(litSrgb)))
        : litSrgb;
    if (surfacePedestal > 0.0) {
        float fgA = clamp((surf.a - surfacePedestal) / max(1.0 - surfacePedestal, 0.001), 0.0, 1.0);
        if (surf.a < surfacePedestal * 0.5)
            fgA = max(fgA, max(surf.r, max(surf.g, surf.b)));
        srgb = clamp(surf.rgb, 0.0, 1.0) + srgb * (1.0 - fgA);
    }
    outputColor = vec4(srgb, 1.0);
}
"""
    parts.append(f'inline constexpr char {name}[] = R"shader({shader})shader";\n')
Path(sys.argv[1]).write_text("".join(parts))
