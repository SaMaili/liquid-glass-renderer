#version 440

// Exact 1:1 GLSL port of Apple macOS 27.2 "Golden Gate" (26B123)
// DesignLibrary.framework/Resources/default.metallib -> SiriEffect.metal
//   mode < 0.5 : siriWaveCore      (4x RGSS MSAA chromatic optical light wave)
//   mode >= 0.5: siriFluidDotsCore (6-pair fluid metaball thinking dots)

layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    vec2  itemSize;          // Shader rect size in pixels
    vec2  anchor;            // Normalized anchor point (default 0.5, 0.5)
    float time;              // Continuous time in seconds
    float phase;             // Wave phase in radians (mod 20*pi)
    float resolved;          // 0.0 = orb/circle, 1.0 = full horizontal wave
    float waveMode;          // 0.0 = siriWaveCore, 1.0 = siriFluidDotsCore (can crossfade)
    float low;               // Low-band audio meter 0..1
    float mid;               // Mid-band audio meter 0..1
    float high;              // High-band audio meter 0..1
    float scale;             // Overall effect scale (default 1.0)
    float useBlack;          // 1.0 = light-mode black fluid dots, 0.0 = glowing white
    float appear;            // Fluid dots appear scale 0..1
    float intensityScale;    // Global brightness multiplier (default 1.0)
    float cornerRadius;      // Pill/circle corner radius in px for clean inner clip
    float circleShape;       // 1.0 = 26B123 SiriWaveShape.circle, 0.0 = .oval
};

const float PI       = 3.14159265359;
const float TWO_PI   = 6.28318530718;
const float HALF_PI  = 1.57079632679;
const float TWENTY_PI = 62.8318530718;

// Exact 26B123 SiriWaveEffect.init() WaveUniforms defaults (0x195fa0690)
const float W_AMPLITUDE                = 0.125;
const float W_FREQ                     = 1.25;
const float W_ABERRATION_FREQ          = 1.25;
const float W_SPEED                    = -1.0;
const float W_SCALE                    = 1.0;
const float W_ABERRATION               = 1.0;
const float W_CENTER_THICKNESS         = 16.0;
const float W_ABERRATION_THICKNESS     = 8.0;
const float W_INTENSITY                = 8.0;
const float W_FALLOFF                  = 2.0;
const float W_EDGE_MASK                = 0.25;
const float W_EDGE_MASK_INSET          = 0.0;
const float W_BAND_FILL                = 0.0;
const float W_BAND_FILL_THICKNESS      = 0.001;
const float W_BLOOM                    = 2.5;
const float W_BLOOM_THICKNESS          = 35.0;
const float W_RED_CHANNEL_SHIFT        = -26.0;
const float W_GREEN_CHANNEL_SHIFT      = -13.0;
const float W_CHROMA_BOOST             = 0.3;
const float W_WAVE_SATURATION          = 2.0;
const float W_AMPLITUDE_TAPER          = 0.8;
const float W_ABERRATION_AMP_TAPER     = 0.8;
const float W_ABERRATION_TAPER         = 0.0;
const float W_FREQ_TAPER               = 0.0;
const float W_LOW_AMPLITUDE            = 42.0;
const float W_LOW_INTENSITY            = 3.5;
const float W_LOW_ABERRATION           = 1.5;
const float W_LOW_ABERRATION_THICKNESS = 0.0;
const float W_LOW_ABERRATION_INTENSITY = 10.0;
const float W_MID_ABERRATION           = 6.0;
const float W_MID_BAND_FILL            = 200.0;
const float W_HIGH_ABERRATION          = 4.0;
const float W_UNRESOLVED_SCALE         = 0.05;

// Exact 26B123 SiriWaveEffect.init() FluidDotsUniforms defaults
const float D_ROTATION        = 0.75;
const float D_RING_RADIUS     = 0.45;
const float D_DOT_RADIUS      = 0.10;
const float D_PAIR_OFFSET     = 0.065;
const float D_PAIR_SMOOTHNESS = 0.20;
const float D_SMOOTHNESS      = 0.25;
const float D_SCALE_DURATION  = 1.10;
const float D_SCALE_STAGGER   = -0.15;
const float D_SCALE_MIN       = 0.10;
const float D_SCALE_MAX       = 0.80;
const float D_GLOW_RADIUS     = 1.00;
const float D_GLOW_STRENGTH   = 0.80;

float sminQuad(float a, float b, float k) {
    if (k <= 0.0001) return min(a, b);
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - 0.25 * k * h * h;
}

float sdRoundedRect(vec2 p, vec2 halfSize, float r) {
    vec2 q = abs(p) - halfSize + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

vec4 evalSiriWave(vec2 fragPx) {
    float r = clamp(resolved, 0.0, 1.0);
    float l = clamp(low, 0.0, 1.0);
    float m = clamp(mid, 0.0, 1.0);
    float h = clamp(high, 0.0, 1.0);

    float phaseMod = phase - trunc(phase / TWENTY_PI) * TWENTY_PI;
    float t = phaseMod * W_SPEED;

    float isCircle = clamp(circleShape, 0.0, 1.0);
    // Exact 26B123 SiriWaveShape.circle overrides (DesignLibrary 0x195e93490)
    float wFreq        = mix(W_FREQ, 1.5, isCircle);
    float wAberrFreq   = mix(W_ABERRATION_FREQ, 1.5, isCircle);
    float wBloom       = mix(W_BLOOM, 3.25, isCircle);
    float wChromaBoost = mix(W_CHROMA_BOOST, 0.25, isCircle);
    float wAmpTaper    = mix(W_AMPLITUDE_TAPER, 1.675, isCircle);
    float wAberrTaper  = mix(W_ABERRATION_AMP_TAPER, 1.675, isCircle);
    float wHighAberr   = mix(W_HIGH_ABERRATION, 2.0, isCircle);
    float wFalloff     = mix(W_FALLOFF, 1.25, isCircle);
    float wAmpBase     = mix(W_AMPLITUDE, 0.16, isCircle);

    float baseIntensity = W_INTENSITY * 0.01;
    float bandFillVal = (W_BAND_FILL + m * W_MID_BAND_FILL) * 0.0001;
    float bloomVal = wBloom * 0.01;
    float effAmp = min(wAmpBase + (l * 0.01) * W_LOW_AMPLITUDE, max(0.36, wAmpBase)) * r;

    float maxAberr = max(4.0, W_ABERRATION);
    float effAberr = min(W_ABERRATION + l * W_LOW_ABERRATION + m * W_MID_ABERRATION + h * wHighAberr, maxAberr) * r;

    float effAberrIntensity = min(baseIntensity + (l * 0.01) * (W_LOW_INTENSITY + W_LOW_ABERRATION_INTENSITY), max(0.2, baseIntensity));
    float effAberrThick = (W_ABERRATION_THICKNESS + l * W_LOW_ABERRATION_THICKNESS) * 0.01;
    float effCenterThick = W_CENTER_THICKNESS * 0.01 + 0.0001;

    float chroma = min(1.0 + (l + m + h) * wChromaBoost, 1.25);
    float aberrFreqScaled = wAberrFreq * 0.01;
    vec3 rgbShift = vec3(
        aberrFreqScaled * W_RED_CHANNEL_SHIFT * chroma,
        aberrFreqScaled * W_GREEN_CHANNEL_SHIFT * chroma,
        0.0
    );

    float unresolvedBoost = mix(4.0 + m * 10.0, 1.0, r);
    float unresolvedCenterBoost = (1.0 - r) * (4.0 + l * 14.0);

    float centerIntensity = mix(0.1, baseIntensity, r) * 0.5;
    float aberrIntensity = mix(0.1, effAberrIntensity, r);
    float cThick = mix(0.1, effCenterThick, r);
    float aThick = mix(0.1, effAberrThick, r);
    float bThick = max(W_BLOOM_THICKNESS * 0.01, 0.0001);
    float bfThick = max(W_BAND_FILL_THICKNESS, 0.0001);

    float effScale = max(W_SCALE * scale, 0.01);
    float aspect = itemSize.x / max(itemSize.y, 1.0);
    float aspectClamped = max(aspect, 1.0);
    float orbRadius = mix(0.14, W_UNRESOLVED_SCALE, r);
    float fTaper = max(W_FREQ_TAPER, 0.0001);
    vec2 anchorNDC = (anchor * 2.0 - 1.0) * vec2(aspect, 1.0);

    vec3 weights[3];
    weights[0] = mix(vec3(1.0), vec3(0.14), vec3(r));
    weights[1] = mix(vec3(1.0), vec3(0.075), vec3(r));
    weights[2] = mix(vec3(1.0), vec3(0.302), vec3(r));
    vec3 weightSum = weights[0] + weights[1] + weights[2];

    float waveOffsets[3];
    waveOffsets[0] = -0.6;
    waveOffsets[1] = 0.0;
    waveOffsets[2] = 1.0;

    vec2 msaaOffsets[4];
    msaaOffsets[0] = vec2(-0.125, -0.375);
    msaaOffsets[1] = vec2( 0.375, -0.125);
    msaaOffsets[2] = vec2(-0.375,  0.125);
    msaaOffsets[3] = vec2( 0.125,  0.375);

    vec3 accum = vec3(0.0);

    for (int s = 0; s < 4; ++s) {
        vec2 uv = ((fragPx + msaaOffsets[s]) * 2.0 / itemSize) - 1.0;
        uv.x *= aspect;
        vec2 p = (uv - anchorNDC) / effScale;

        float orbDist = max(length(p) - orbRadius, 0.0);
        float xn = p.x / aspectClamped;

        float cAmp = cos(min(abs(xn * wAmpTaper), 1.0) * HALF_PI);
        float cAberrAmp = cos(min(abs(xn * wAberrTaper), 1.0) * HALF_PI);
        float aberrDiv = 1.0 + pow(abs(xn * W_ABERRATION_TAPER), 2.0);
        float xw = atan(xn * fTaper) * aspectClamped / fTaper;

        float centerWaveY = (cAmp * cAmp * effAmp) * sin(xw * wFreq + t);
        float aberrAmp = cAberrAmp * cAberrAmp * effAmp;
        float aberrBasePhase = xw * wAberrFreq + t;

        vec3 aberrCol = vec3(0.0);
        for (int i = 0; i < 3; ++i) {
            vec3 ph = vec3(aberrBasePhase + (effAberr * waveOffsets[i]) / aberrDiv) + rgbShift;
            vec3 wy = sin(ph) * aberrAmp;
            vec3 d3 = mix(vec3(orbDist), abs(vec3(p.y) - wy), vec3(r));

            vec3 lor = mix(1.0 / (1.0 + (d3 * 0.02) * (d3 * 0.02)), vec3(1.0), vec3(r));
            vec3 line3 = (lor * aberrIntensity) / (d3 + aThick);

            vec3 lo3 = min(vec3(centerWaveY), wy);
            vec3 hi3 = max(vec3(centerWaveY), wy);
            vec3 bfDist = max(vec3(0.0), max(vec3(p.y) - hi3, lo3 - vec3(p.y)));
            vec3 bf3 = min((aberrIntensity * bandFillVal) / (bfDist + bfThick), vec3(0.5)) * r;

            vec3 bloom3 = (r * bloomVal) / (d3 + bThick);

            aberrCol += (weights[i] * unresolvedBoost) * (line3 + bloom3 + bf3);
        }
        aberrCol /= weightSum;

        float dc = mix(orbDist, abs(p.y - centerWaveY), r);
        float lorC = mix(1.0 / (1.0 + (dc * 0.02) * (dc * 0.02)), 1.0, r) + unresolvedCenterBoost;
        float core = (centerIntensity * lorC) / (dc + cThick);
        float coreBloom = (wBloom * 0.005 * r) / (dc + bThick);

        vec3 sampleCol = pow(max(aberrCol + vec3(core + coreBloom), vec3(0.0)), vec3(1.5));
        accum += sampleCol;
    }

    // Center-pixel vertical edge mask + horizontal Gaussian falloff
    vec2 uv0 = (fragPx * 2.0 / itemSize) - 1.0;
    uv0.x *= aspect;
    vec2 p0 = (uv0 - anchorNDC) / effScale;

    float em = max(clamp(W_EDGE_MASK, 0.0, 1.0), 0.0001);
    float emi = clamp(W_EDGE_MASK_INSET, 0.0, 1.0);
    float et = clamp((abs(p0.y) - 1.0 + emi) / (-em), 0.0, 1.0);
    float vMask = et * et * (3.0 - 2.0 * et);

    float fx = (p0.x * wFalloff) / aspectClamped;
    float hMask = exp(-(fx * fx));
    float mask = mix(1.0, vMask * hMask, r);

    vec3 col = (accum * mask) * 0.25;

    // Rec.709 saturation boost
    float luma = dot(col, vec3(0.2126, 0.7152, 0.0722));
    col = max(mix(vec3(luma), col, vec3(W_WAVE_SATURATION)), vec3(0.0));
    col *= r * intensityScale;

    // Soft tone-mapping to keep vibrant chromatic fringes while preserving white core
    vec3 outRgb = 1.0 - exp(-col * 1.15);
    float outA = max(outRgb.r, max(outRgb.g, outRgb.b));
    return vec4(outRgb, clamp(outA, 0.0, 1.0));
}

vec4 evalFluidDots(vec2 fragPx) {
    float minDim = min(itemSize.x, itemSize.y);
    float halfMin = max(minDim * 0.5, 1.0);
    vec2 anchorOff = vec2(itemSize.x / halfMin, 2.0) * (anchor - 0.5);
    float effScale = max(scale, 0.001);
    vec2 p = (((fragPx - 0.5 * itemSize) / halfMin) - anchorOff) / effScale;

    float tMod = time - trunc(time / TWENTY_PI) * TWENTY_PI;
    float rotAngle = tMod * D_ROTATION;
    float dur = max(D_SCALE_DURATION, 0.001);

    float dAll = 1e9;
    float glowAccum = 0.0;

    for (int i = 0; i < 6; ++i) {
        float fi = float(i);
        float ang = fi * 1.04719758 + rotAngle;
        vec2 dir = vec2(cos(ang), sin(ang));
        vec2 perp = vec2(-dir.y, dir.x);

        float pulse = fract((time + fi * D_SCALE_STAGGER) / dur);
        float dotScale = mix(D_SCALE_MIN, D_SCALE_MAX, 0.5 - 0.5 * cos(pulse * TWO_PI));

        vec2 basePos = dir * (appear * D_RING_RADIUS);
        vec2 offPos = perp * (appear * D_PAIR_OFFSET * dotScale);
        vec2 c1 = basePos - offPos;
        vec2 c2 = basePos + offPos;

        float rad = dotScale * D_DOT_RADIUS;
        float d1 = length(p - c1) - rad;
        float d2 = length(p - c2) - rad;

        float dp = sminQuad(d1, d2, D_PAIR_SMOOTHNESS);
        dAll = sminQuad(dAll, dp, D_SMOOTHNESS);

        float gDen = max(rad * D_GLOW_RADIUS, 0.0001);
        float u1 = max(d1, 0.0) / gDen;
        float w1 = clamp(u1 * 0.16666667, 0.0, 1.0);
        float g1 = (1.0 - (w1 * w1) * (w1 * w1)) / (1.0 + u1);

        float u2 = max(d2, 0.0) / gDen;
        float w2 = clamp(u2 * 0.16666667, 0.0, 1.0);
        float g2 = (1.0 - (w2 * w2) * (w2 * w2)) / (1.0 + u2);

        glowAccum += g1 * g1 + g2 * g2;
    }

    float edge = 2.0 / max(effScale * minDim, 1.0);
    float shape = 1.0 - smoothstep(-edge, edge, dAll);

    if (useBlack > 0.5) {
        return vec4(0.0, 0.0, 0.0, clamp(shape, 0.0, 1.0));
    }
    float v = 1.0 - exp(-(shape * 4.0 + glowAccum * D_GLOW_STRENGTH));
    return vec4(v, v, v, clamp(v, 0.0, 1.0));
}

void main() {
    vec2 fragPx = qt_TexCoord0 * itemSize;

    // Anti-aliased pill clip so glow never leaks past the rounded capsule
    float clipMask = 1.0;
    if (cornerRadius > 0.5) {
        float dPill = sdRoundedRect(fragPx - itemSize * 0.5, itemSize * 0.5, min(cornerRadius, min(itemSize.x, itemSize.y) * 0.5));
        clipMask = 1.0 - smoothstep(-0.75, 0.75, dPill);
    }
    if (clipMask <= 0.001) {
        fragColor = vec4(0.0);
        return;
    }

    float m = clamp(waveMode, 0.0, 1.0);
    vec4 outCol;
    if (m <= 0.001) {
        outCol = evalSiriWave(fragPx);
    } else if (m >= 0.999) {
        outCol = evalFluidDots(fragPx);
    } else {
        vec4 wCol = evalSiriWave(fragPx);
        vec4 dCol = evalFluidDots(fragPx);
        outCol = mix(wCol, dCol, m);
    }

    fragColor = outCol * (clipMask * qt_Opacity);
}
