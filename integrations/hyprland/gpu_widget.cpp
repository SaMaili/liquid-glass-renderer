#include "gpu_widget.hpp"
#include "widget_shaders.hpp"

#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <list>
#include <stdexcept>
#include <string>

namespace {
constexpr GLuint sourceUnit = 3, sdfUnit = 4, maskUnit = 5;
constexpr std::array<GLuint, 4> bindings{1, 6, 9, 10};
constexpr std::array<GLenum, 7> capabilities{
    GL_BLEND, GL_SCISSOR_TEST, GL_DEPTH_TEST, GL_STENCIL_TEST, GL_CULL_FACE, GL_RASTERIZER_DISCARD, GL_DITHER,
};

class GlState {
    GLint read, draw, active, vao, program, uniformBuffer;
    GLint viewport[4], scissor[4], textures[3], samplers[3], uniforms[4];
    GLint64 offsets[4], sizes[4];
    GLboolean flags[capabilities.size()], colorMask[4];
  public:
    GlState() {
        glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &read);
        glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &draw);
        glGetIntegerv(GL_ACTIVE_TEXTURE, &active);
        glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &vao);
        glGetIntegerv(GL_CURRENT_PROGRAM, &program);
        glGetIntegerv(GL_UNIFORM_BUFFER_BINDING, &uniformBuffer);
        glGetIntegerv(GL_VIEWPORT, viewport);
        glGetIntegerv(GL_SCISSOR_BOX, scissor);
        glGetBooleanv(GL_COLOR_WRITEMASK, colorMask);
        glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
        for (size_t i = 0; i < capabilities.size(); ++i) {
            flags[i] = glIsEnabled(capabilities[i]);
            glDisable(capabilities[i]);
        }
        for (GLuint i = 0; i < 3; ++i) {
            glActiveTexture(GL_TEXTURE0 + sourceUnit + i);
            glGetIntegerv(GL_TEXTURE_BINDING_2D, &textures[i]);
            glGetIntegerv(GL_SAMPLER_BINDING, &samplers[i]);
            glBindSampler(sourceUnit + i, 0);
        }
        for (size_t i = 0; i < bindings.size(); ++i) {
            glGetIntegeri_v(GL_UNIFORM_BUFFER_BINDING, bindings[i], &uniforms[i]);
            glGetInteger64i_v(GL_UNIFORM_BUFFER_START, bindings[i], &offsets[i]);
            glGetInteger64i_v(GL_UNIFORM_BUFFER_SIZE, bindings[i], &sizes[i]);
        }
    }
    ~GlState() {
        for (size_t i = 0; i < bindings.size(); ++i) {
            if (uniforms[i] && sizes[i] > 0)
                glBindBufferRange(GL_UNIFORM_BUFFER, bindings[i], uniforms[i], offsets[i], sizes[i]);
            else
                glBindBufferBase(GL_UNIFORM_BUFFER, bindings[i], uniforms[i]);
        }
        glBindBuffer(GL_UNIFORM_BUFFER, uniformBuffer);
        for (GLuint i = 0; i < 3; ++i) {
            glActiveTexture(GL_TEXTURE0 + sourceUnit + i);
            glBindTexture(GL_TEXTURE_2D, textures[i]);
            glBindSampler(sourceUnit + i, samplers[i]);
        }
        glActiveTexture(active);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, read);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, draw);
        glUseProgram(program);
        glBindVertexArray(vao);
        glViewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        glScissor(scissor[0], scissor[1], scissor[2], scissor[3]);
        glColorMask(colorMask[0], colorMask[1], colorMask[2], colorMask[3]);
        for (size_t i = 0; i < capabilities.size(); ++i)
            if (flags[i]) glEnable(capabilities[i]);
    }
};

constexpr char vertex[] = R"(#version 320 es
layout(location=0) out highp vec2 sourceUV;
layout(location=1) out highp vec2 sdfUV;
void main() {
    vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    sourceUV = p;
    sdfUV = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0, 1);
})";
constexpr char shapeVertex[] = R"(#version 320 es
layout(location=0) out highp vec2 shape;
layout(location=1) out highp vec2 tip;
layout(std140,binding=10) uniform ShapeTransform { vec4 row0; vec4 row1; vec4 tip0; vec4 tip1; } transform;
void main() {
    vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    shape = vec2(dot(vec3(p,1),transform.row0.xyz),dot(vec3(p,1),transform.row1.xyz));
    tip = vec2(dot(vec3(p,1),transform.tip0.xyz),dot(vec3(p,1),transform.tip1.xyz));
    gl_Position = vec4(p * 2.0 - 1.0, 0, 1);
})";
constexpr char decodeShader[] = R"(#version 320 es
precision highp float;
precision highp sampler2D;
layout(binding=3) uniform sampler2D encoded;
layout(location=0) out vec4 result;
void main() {
    vec3 srgb = texelFetch(encoded, ivec2(gl_FragCoord.xy), 0).rgb;
    vec3 linearRGB = mix(pow((srgb+0.055)/1.055,vec3(2.4)),srgb/12.92,
                         lessThanEqual(srgb,vec3(0.04045)));
    result = vec4(linearRGB,1);
})";

constexpr char jfaSeedShader[] = R"(#version 320 es
precision highp float;
precision highp sampler2D;
layout(binding=5) uniform sampler2D surfaceMask;
uniform ivec2 maskOffset;
uniform ivec2 cropSize;
uniform float alphaThreshold;
layout(location=0) out vec2 seedOut;

float sampleAlpha(ivec2 p) {
    ivec2 c = clamp(p, ivec2(0), cropSize - ivec2(1));
    return texelFetch(surfaceMask, maskOffset + c, 0).a;
}

void main() {
    ivec2 hp = ivec2(gl_FragCoord.xy);
    ivec2 fp0 = hp * 2;
    float a00 = sampleAlpha(fp0);
    float a10 = sampleAlpha(fp0 + ivec2(1, 0));
    float a01 = sampleAlpha(fp0 + ivec2(0, 1));
    float a11 = sampleAlpha(fp0 + ivec2(1, 1));
    float aCenter = 0.25 * (a00 + a10 + a01 + a11);
    bool inCenter = aCenter >= alphaThreshold;

    bool crosses = (a00 >= alphaThreshold) != inCenter
                || (a10 >= alphaThreshold) != inCenter
                || (a01 >= alphaThreshold) != inCenter
                || (a11 >= alphaThreshold) != inCenter;
    if (!crosses) {
        ivec2 c = fp0 + ivec2(1, 1);
        crosses = (sampleAlpha(c + ivec2(-2, 0)) >= alphaThreshold) != inCenter
               || (sampleAlpha(c + ivec2( 2, 0)) >= alphaThreshold) != inCenter
               || (sampleAlpha(c + ivec2( 0,-2)) >= alphaThreshold) != inCenter
               || (sampleAlpha(c + ivec2( 0, 2)) >= alphaThreshold) != inCenter;
    }
    if (!crosses) {
        seedOut = vec2(-1e5);
        return;
    }
    vec2 centerPos = vec2(fp0) + vec2(1.0);
    float aL = sampleAlpha(fp0 + ivec2(-1, 0)) + sampleAlpha(fp0 + ivec2(-1, 1));
    float aR = sampleAlpha(fp0 + ivec2( 2, 0)) + sampleAlpha(fp0 + ivec2( 2, 1));
    float aU = sampleAlpha(fp0 + ivec2( 0,-1)) + sampleAlpha(fp0 + ivec2( 1,-1));
    float aD = sampleAlpha(fp0 + ivec2( 0, 2)) + sampleAlpha(fp0 + ivec2( 1, 2));
    vec2 grad = vec2(aR - aL, aD - aU) * 0.25;
    float gLen = length(grad);
    vec2 offset = gLen > 0.01 ? (grad / gLen) * clamp((alphaThreshold - aCenter) / gLen, -1.5, 1.5) : vec2(0.0);
    seedOut = centerPos + offset;
})";

constexpr char jfaStepShader[] = R"(#version 320 es
precision highp float;
precision highp sampler2D;
layout(binding=3) uniform sampler2D jfaPrev;
uniform int jfaStep;
layout(location=0) out vec2 seedOut;

void main() {
    ivec2 hp = ivec2(gl_FragCoord.xy);
    ivec2 sz = textureSize(jfaPrev, 0) - ivec2(1);
    vec2 fp = vec2(hp * 2) + vec2(1.0);
    vec2 best = vec2(-1e5);
    float bestD2 = 1e20;
    for (int dy = -1; dy <= 1; ++dy) {
        for (int dx = -1; dx <= 1; ++dx) {
            ivec2 q = clamp(hp + ivec2(dx, dy) * jfaStep, ivec2(0), sz);
            vec2 s = texelFetch(jfaPrev, q, 0).rg;
            if (s.x > -1e4) {
                vec2 d = fp - s;
                float d2 = dot(d, d);
                if (d2 < bestD2) {
                    bestD2 = d2;
                    best = s;
                }
            }
        }
    }
    seedOut = best;
})";

constexpr char jfaFinalizeShader[] = R"(#version 320 es
precision highp float;
precision highp sampler2D;
layout(binding=3) uniform sampler2D jfaFinal;
layout(binding=5) uniform sampler2D surfaceMask;
uniform ivec2 maskOffset;
uniform ivec2 cropSize;
uniform float alphaThreshold;
uniform float invScale;
layout(location=0) out vec4 sdfOut;

float sampleAlpha(ivec2 p) {
    ivec2 c = clamp(p, ivec2(0), cropSize - ivec2(1));
    return texelFetch(surfaceMask, maskOffset + c, 0).a;
}

void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec2 fp = vec2(p) + vec2(0.5);
    float a = sampleAlpha(p);
    bool inside = a >= alphaThreshold;

    ivec2 hp = p / 2;
    ivec2 jfaMax = textureSize(jfaFinal, 0) - ivec2(1);
    vec2 best = vec2(-1e5);
    float bestD2 = 1e20;
    for (int dy = -1; dy <= 1; ++dy) {
        for (int dx = -1; dx <= 1; ++dx) {
            ivec2 q = clamp(hp + ivec2(dx, dy), ivec2(0), jfaMax);
            vec2 s = texelFetch(jfaFinal, q, 0).rg;
            if (s.x > -1e4) {
                vec2 d = fp - s;
                float d2 = dot(d, d);
                if (d2 < bestD2) {
                    bestD2 = d2;
                    best = s;
                }
            }
        }
    }

    float signedPx;
    vec2 normal;
    if (best.x > -1e4) {
        vec2 delta = fp - best;
        float dPx = sqrt(bestD2);
        signedPx = inside ? -dPx : dPx;
        normal = dPx > 1e-4 ? (inside ? -delta : delta) / dPx : vec2(0.0, -1.0);
        if (dPx < 3.0) {
            float aL = sampleAlpha(p + ivec2(-1, 0));
            float aR = sampleAlpha(p + ivec2( 1, 0));
            float aU = sampleAlpha(p + ivec2( 0,-1));
            float aD = sampleAlpha(p + ivec2( 0, 1));
            vec2 g = vec2(aR - aL, aD - aU) * 0.5;
            float gLen = length(g);
            if (gLen > 0.02) {
                float alphaDist = clamp((alphaThreshold - a) / gLen, -2.5, 2.5);
                float t = smoothstep(0.5, 2.5, dPx);
                signedPx = mix(alphaDist, signedPx, t);
                vec2 gn = -g / gLen;
                normal = normalize(mix(gn, normal, t));
            }
        }
    } else {
        signedPx = inside ? -128.0 : 128.0;
        normal = vec2(0.0, -1.0);
    }
    sdfOut = vec4(signedPx * invScale, normal.x, normal.y, 1.0);
})";

GLuint compile(GLenum kind, const char* source) {
    GLuint shader = glCreateShader(kind);
    glShaderSource(shader, 1, &source, nullptr);
    glCompileShader(shader);
    GLint fine;
    glGetShaderiv(shader, GL_COMPILE_STATUS, &fine);
    if (fine) return shader;
    char message[2048]{};
    glGetShaderInfoLog(shader, sizeof(message), nullptr, message);
    glDeleteShader(shader);
    throw std::runtime_error(message);
}

class Program {
  public:
    GLuint id = 0;
    Program(const char* vert, const char* frag) {
        const GLuint fragment = compile(GL_FRAGMENT_SHADER, frag);
        GLuint vs;
        try { vs = compile(GL_VERTEX_SHADER, vert); }
        catch (...) { glDeleteShader(fragment); throw; }
        id = glCreateProgram();
        glAttachShader(id, vs);
        glAttachShader(id, fragment);
        glLinkProgram(id);
        glDeleteShader(vs);
        glDeleteShader(fragment);
        GLint fine;
        glGetProgramiv(id, GL_LINK_STATUS, &fine);
        if (fine) return;
        char message[2048]{};
        glGetProgramInfoLog(id, sizeof(message), nullptr, message);
        glDeleteProgram(id);
        throw std::runtime_error(message);
    }
    ~Program() { glDeleteProgram(id); }
};

void bind(GLuint unit, GLuint name) {
    glActiveTexture(GL_TEXTURE0 + unit);
    glBindTexture(GL_TEXTURE_2D, name);
}

struct Images {
    int width, height, levels;
    // Encoded input/output (RGBA8), linear mip chain (RGBA16F), SDF (RGBA32F), downsample temporary (RGBA16F).
    std::array<GLuint, 4> texture{};
    std::array<GLuint, 2> jfa{};
    int jfaW = 0, jfaH = 0;
    bool hasSdf = false;
    bool lastCartouche = false;
    float lastGeometry[6][4]{};
    float lastTransform[4][4]{};
    int lastMitosisCount = 0;
    float lastMitosisSmoothness = 0.F;
    std::array<std::array<float, 4>, 4> lastMitosisCircles{};
    Images(int w, int h, int n, GLuint fbo) : width(w), height(h), levels(n) {
        glGenTextures(texture.size(), texture.data());
        for (size_t i = 0; i < texture.size(); ++i) {
            bind(sourceUnit, texture[i]);
            const GLenum format = i == 0 ? GL_RGBA8 : (i == 2 ? GL_RGBA32F : GL_RGBA16F);
            glTexStorage2D(GL_TEXTURE_2D, i == 1 ? n : 1, format,
                           i == 3 ? std::max(w / 2, 1) : w, i == 3 ? std::max(h / 2, 1) : h);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, fbo);
            glFramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture[i], 0);
            if (glCheckFramebufferStatus(GL_DRAW_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
                throw std::runtime_error("widget GPU framebuffer is incomplete");
        }
    }
    void ensureJfa(GLuint fbo) {
        if (jfa[0] != 0)
            return;
        jfaW = std::max((width + 1) / 2, 1);
        jfaH = std::max((height + 1) / 2, 1);
        glGenTextures(jfa.size(), jfa.data());
        for (size_t i = 0; i < jfa.size(); ++i) {
            bind(sourceUnit, jfa[i]);
            glTexStorage2D(GL_TEXTURE_2D, 1, GL_RG32F, jfaW, jfaH);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, fbo);
            glFramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, jfa[i], 0);
            if (glCheckFramebufferStatus(GL_DRAW_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
                throw std::runtime_error("widget JFA framebuffer is incomplete");
        }
    }
    ~Images() {
        glDeleteTextures(texture.size(), texture.data());
        if (jfa[0] != 0)
            glDeleteTextures(jfa.size(), jfa.data());
    }
};
}

struct GpuWidgets::Resources {
    EGLContext context = eglGetCurrentContext();
    Program decode{vertex, decodeShader};
    Program distance{shapeVertex, distanceShader};
    Program blur{vertex, blurShader};
    Program material{vertex, materialShader};
    Program jfaSeed{vertex, jfaSeedShader};
    Program jfaStep{vertex, jfaStepShader};
    Program jfaFinalize{vertex, jfaFinalizeShader};
    GLint opacity = glGetUniformLocation(material.id, "widgetOpacity");
    GLint cartouche = glGetUniformLocation(distance.id, "cartouche");
    GLint mitosisCount = glGetUniformLocation(distance.id, "mitosisCount");
    GLint mitosisSmoothness = glGetUniformLocation(distance.id, "mitosisSmoothness");
    GLint mitosisInvScale = glGetUniformLocation(distance.id, "mitosisInvScale");
    GLint mitosisCircles = glGetUniformLocation(distance.id, "mitosisCircles[0]");
    GLint legibility = glGetUniformLocation(material.id, "legibilityLod");
    GLint dockSrgbFace = glGetUniformLocation(material.id, "dockSrgbFace");
    GLint plate = glGetUniformLocation(material.id, "widgetPlate[0]");
    GLint tint = glGetUniformLocation(material.id, "widgetTint");
    GLint tint2 = glGetUniformLocation(material.id, "widgetTint2");
    GLint tintAxis = glGetUniformLocation(material.id, "widgetTintAxis");
    GLint highlightMatrix = glGetUniformLocation(material.id, "highlightMatrix[0]");
    GLint highlightLobes = glGetUniformLocation(material.id, "highlightLobes[0]");
    GLint surfacePedestal = glGetUniformLocation(material.id, "surfacePedestal");
    GLint maskOffset = glGetUniformLocation(material.id, "maskOffset");
    GLint sourceLevel = glGetUniformLocation(blur.id, "sourceLevel");
    GLint seedOffset = glGetUniformLocation(jfaSeed.id, "maskOffset");
    GLint seedSize = glGetUniformLocation(jfaSeed.id, "cropSize");
    GLint seedThreshold = glGetUniformLocation(jfaSeed.id, "alphaThreshold");
    GLint stepUniform = glGetUniformLocation(jfaStep.id, "jfaStep");
    GLint finOffset = glGetUniformLocation(jfaFinalize.id, "maskOffset");
    GLint finSize = glGetUniformLocation(jfaFinalize.id, "cropSize");
    GLint finThreshold = glGetUniformLocation(jfaFinalize.id, "alphaThreshold");
    GLint finInvScale = glGetUniformLocation(jfaFinalize.id, "invScale");
    GLuint vao, fbo[2], ubo[4];
    std::array<size_t, 4> uboCapacity{};
    std::list<Images> cache;
    uint64_t misses = 0;
    uint64_t sdfSkipped = 0;

    Resources() {
        glGenVertexArrays(1, &vao);
        glGenFramebuffers(2, fbo);
        glGenBuffers(4, ubo);
    }
    ~Resources() {
        glDeleteVertexArrays(1, &vao);
        glDeleteFramebuffers(2, fbo);
        glDeleteBuffers(4, ubo);
    }
    void uniform(size_t index, const void* bytes, size_t length) {
        glBindBuffer(GL_UNIFORM_BUFFER, ubo[index]);
        if (uboCapacity[index] < length) {
            glBufferData(GL_UNIFORM_BUFFER, length, bytes, GL_DYNAMIC_DRAW);
            uboCapacity[index] = length;
        } else {
            glBufferSubData(GL_UNIFORM_BUFFER, 0, length, bytes);
        }
        glBindBufferBase(GL_UNIFORM_BUFFER, bindings[index], ubo[index]);
    }
    void output(GLuint tex, int level = 0) {
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, fbo[0]);
        glFramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, level);
    }
    void run(GLuint program, int w, int h) {
        glUseProgram(program);
        glViewport(0, 0, w, h);
        glDrawArrays(GL_TRIANGLES, 0, 3);
    }
    Images& images(const Widget& params, int levels) {
        auto found = std::find_if(cache.begin(), cache.end(), [&](const Images& i) {
            return i.width == static_cast<int>(params.width) && i.height == static_cast<int>(params.height) && i.levels == levels;
        });
        if (found != cache.end())
            cache.splice(cache.begin(), cache, found);
        else {
            ++misses;
            if (cache.size() == 16) cache.pop_back();
            cache.emplace_front(params.width, params.height, levels, fbo[0]);
        }
        return cache.front();
    }
};

GpuWidgets::GpuWidgets() = default;
GpuWidgets::~GpuWidgets() {
    if (resources && resources->context != eglGetCurrentContext())
        resources.release(); // This context owns the objects and frees them at teardown.
}
uint64_t GpuWidgets::cacheMisses() const { return resources ? resources->misses : 0; }
uint64_t GpuWidgets::sdfSkips() const { return resources ? resources->sdfSkipped : 0; }

bool GpuWidgets::draw(GLuint target, FramebufferRegion crop, FramebufferRegion widget, const Widget& params,
                      const MitosisShapes& mitosis) {
    if (reported || eglGetCurrentContext() == EGL_NO_CONTEXT ||
        (resources && resources->context != eglGetCurrentContext()))
        return false;
    PreparedWidget data;
    if (!lg_widget_prepare(&params, &data))
        return false;
    try {
        const GlState saved;
        if (!resources)
            resources = std::make_unique<Resources>();
        auto& gpu = *resources;
        auto& images = gpu.images(params, data.levels);
        const auto [encoded, linear, sdf, temporary] = images.texture;
        const int w = images.width, h = images.height;
        glBindVertexArray(gpu.vao);
        gpu.output(encoded);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, target);
        copyToWidgetTexture(crop);

        gpu.output(linear, 0);
        bind(sourceUnit, encoded);
        gpu.run(gpu.decode.id, w, h);

        bind(sourceUnit, linear);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_NEAREST);
        for (int level = 1, mw = w, mh = h; level < images.levels; ++level) {
            mw = std::max(1, mw / 2);
            mh = std::max(1, mh / 2);
            const float step[4]{1.F / mw, 1.F / mh, 0, 0};
            gpu.output(temporary);
            gpu.uniform(0, step, sizeof(step));
            glUseProgram(gpu.blur.id);
            glUniform1f(gpu.sourceLevel, static_cast<float>(level - 1));
            gpu.run(gpu.blur.id, mw, mh);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, gpu.fbo[1]);
            glFramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, linear, level);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, gpu.fbo[0]);
            glBlitFramebuffer(0, 0, mw, mh, 0, 0, mw, mh, GL_COLOR_BUFFER_BIT, GL_NEAREST);
        }

        const bool isCartouche = params.role == 5;
        const bool sameMitosis = images.lastMitosisCount == mitosis.count
            && images.lastMitosisSmoothness == mitosis.smoothness
            && (mitosis.count == 0 || images.lastMitosisCircles == mitosis.circles);
        if (images.hasSdf && images.lastCartouche == isCartouche && sameMitosis
            && std::memcmp(images.lastGeometry, data.geometry, sizeof(data.geometry)) == 0
            && std::memcmp(images.lastTransform, data.transform, sizeof(data.transform)) == 0) {
            ++gpu.sdfSkipped;
        } else {
            gpu.output(sdf);
            gpu.uniform(0, data.geometry, sizeof(data.geometry));
            gpu.uniform(3, data.transform, sizeof(data.transform));
            glUseProgram(gpu.distance.id);
            glUniform1i(gpu.cartouche, isCartouche);
            glUniform1i(gpu.mitosisCount, std::clamp(mitosis.count, 0, 4));
            glUniform1f(gpu.mitosisSmoothness, mitosis.smoothness);
            glUniform1f(gpu.mitosisInvScale, 1.0F / params.scale);
            glUniform4fv(gpu.mitosisCircles, 4, &mitosis.circles[0][0]);
            gpu.run(gpu.distance.id, w, h);
            std::memcpy(images.lastGeometry, data.geometry, sizeof(data.geometry));
            std::memcpy(images.lastTransform, data.transform, sizeof(data.transform));
            images.lastCartouche = isCartouche;
            images.lastMitosisCount = mitosis.count;
            images.lastMitosisSmoothness = mitosis.smoothness;
            images.lastMitosisCircles = mitosis.circles;
            images.hasSdf = true;
        }

        gpu.output(encoded);
        bind(sourceUnit, linear);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_LINEAR);
        bind(sdfUnit, sdf);
        const float edr[4]{1, 0, 0, 0};
        gpu.uniform(0, data.uniforms, sizeof(data.uniforms));
        gpu.uniform(1, edr, sizeof(edr));
        gpu.uniform(2, data.extension, sizeof(data.extension));
        glUseProgram(gpu.material.id);
        glUniform1f(gpu.opacity, params.opacity);
        glUniform1f(gpu.legibility, data.legibilityLod);
        glUniform1i(gpu.dockSrgbFace, params.role == 3);
        glUniform4fv(gpu.plate, 3, &data.widgetPlate[0][0]);
        glUniform4fv(gpu.tint, 1, data.widgetTint);
        glUniform4fv(gpu.tint2, 1, data.widgetTint2);
        glUniform4fv(gpu.tintAxis, 1, data.widgetTintAxis);
        glUniform4fv(gpu.highlightMatrix, 4, &data.highlightMatrix[0][0]);
        glUniform4fv(gpu.highlightLobes, 2, &data.highlightLobes[0][0]);
        glUniform1f(gpu.surfacePedestal, 0.0F);
        glUniform2i(gpu.maskOffset, 0, 0);
        // Only shade the actual widget pixels inside the padded crop; outer padding pixels
        // are never copied back by copyToCompositor.
        glEnable(GL_SCISSOR_TEST);
        glScissor(widget.left - crop.left, widget.top - crop.top,
                  widget.right - widget.left, widget.bottom - widget.top);
        gpu.run(gpu.material.id, w, h);
        glDisable(GL_SCISSOR_TEST);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, gpu.fbo[0]);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, target);
        copyToCompositor(crop, widget);
        return true;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Liquid Glass GPU: %s\n", error.what());
        reported = true;
        return false;
    }
}

bool GpuWidgets::drawAlphaMask(GLuint target, GLuint surfaceMaskTex, float pedestal,
                               FramebufferRegion crop, FramebufferRegion widget, const Widget& params) {
    if (reported || surfaceMaskTex == 0 || pedestal <= 0.0F || pedestal >= 0.99F
        || eglGetCurrentContext() == EGL_NO_CONTEXT
        || (resources && resources->context != eglGetCurrentContext()))
        return false;
    PreparedWidget data;
    if (!lg_widget_prepare(&params, &data))
        return false;
    try {
        const GlState saved;
        if (!resources)
            resources = std::make_unique<Resources>();
        auto& gpu = *resources;
        auto& images = gpu.images(params, data.levels);
        images.ensureJfa(gpu.fbo[0]);
        const auto [encoded, linear, sdf, temporary] = images.texture;
        const int w = images.width, h = images.height;
        const float threshold = pedestal * 0.5F;
        glBindVertexArray(gpu.vao);

        gpu.output(encoded);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, target);
        copyToWidgetTexture(crop);

        gpu.output(linear, 0);
        bind(sourceUnit, encoded);
        gpu.run(gpu.decode.id, w, h);

        bind(sourceUnit, linear);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_NEAREST);
        for (int level = 1, mw = w, mh = h; level < images.levels; ++level) {
            mw = std::max(1, mw / 2);
            mh = std::max(1, mh / 2);
            const float step[4]{1.F / mw, 1.F / mh, 0, 0};
            gpu.output(temporary);
            gpu.uniform(0, step, sizeof(step));
            glUseProgram(gpu.blur.id);
            glUniform1f(gpu.sourceLevel, static_cast<float>(level - 1));
            gpu.run(gpu.blur.id, mw, mh);
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, gpu.fbo[1]);
            glFramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, linear, level);
            glBindFramebuffer(GL_READ_FRAMEBUFFER, gpu.fbo[0]);
            glBlitFramebuffer(0, 0, mw, mh, 0, 0, mw, mh, GL_COLOR_BUFFER_BIT, GL_NEAREST);
        }

        // Seed and run half-res Jump-Flooding SDF from the captured surface alpha.
        bind(maskUnit, surfaceMaskTex);
        gpu.output(images.jfa[0]);
        glUseProgram(gpu.jfaSeed.id);
        glUniform2i(gpu.seedOffset, crop.left, crop.top);
        glUniform2i(gpu.seedSize, w, h);
        glUniform1f(gpu.seedThreshold, threshold);
        gpu.run(gpu.jfaSeed.id, images.jfaW, images.jfaH);

        int srcIdx = 0;
        for (int step = 32; step >= 1; step >>= 1) {
            const int dstIdx = 1 - srcIdx;
            gpu.output(images.jfa[dstIdx]);
            bind(sourceUnit, images.jfa[srcIdx]);
            glUseProgram(gpu.jfaStep.id);
            glUniform1i(gpu.stepUniform, step);
            gpu.run(gpu.jfaStep.id, images.jfaW, images.jfaH);
            srcIdx = dstIdx;
        }

        gpu.output(sdf);
        bind(sourceUnit, images.jfa[srcIdx]);
        bind(maskUnit, surfaceMaskTex);
        glUseProgram(gpu.jfaFinalize.id);
        glUniform2i(gpu.finOffset, crop.left, crop.top);
        glUniform2i(gpu.finSize, w, h);
        glUniform1f(gpu.finThreshold, threshold);
        glUniform1f(gpu.finInvScale, 1.0F / params.scale);
        gpu.run(gpu.jfaFinalize.id, w, h);
        images.hasSdf = false;

        gpu.output(encoded);
        bind(sourceUnit, linear);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_LINEAR);
        bind(sdfUnit, sdf);
        bind(maskUnit, surfaceMaskTex);
        const float edr[4]{1, 0, 0, 0};
        gpu.uniform(0, data.uniforms, sizeof(data.uniforms));
        gpu.uniform(1, edr, sizeof(edr));
        gpu.uniform(2, data.extension, sizeof(data.extension));
        glUseProgram(gpu.material.id);
        glUniform1f(gpu.opacity, params.opacity);
        glUniform1f(gpu.legibility, data.legibilityLod);
        glUniform1i(gpu.dockSrgbFace, 0);
        glUniform4fv(gpu.plate, 3, &data.widgetPlate[0][0]);
        glUniform4fv(gpu.tint, 1, data.widgetTint);
        glUniform4fv(gpu.tint2, 1, data.widgetTint2);
        glUniform4fv(gpu.tintAxis, 1, data.widgetTintAxis);
        glUniform4fv(gpu.highlightMatrix, 4, &data.highlightMatrix[0][0]);
        glUniform4fv(gpu.highlightLobes, 2, &data.highlightLobes[0][0]);
        glUniform1f(gpu.surfacePedestal, pedestal);
        glUniform2i(gpu.maskOffset, crop.left, crop.top);
        glEnable(GL_SCISSOR_TEST);
        glScissor(widget.left - crop.left, widget.top - crop.top,
                  widget.right - widget.left, widget.bottom - widget.top);
        gpu.run(gpu.material.id, w, h);
        glDisable(GL_SCISSOR_TEST);
        glBindFramebuffer(GL_READ_FRAMEBUFFER, gpu.fbo[0]);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, target);
        copyToCompositor(crop, widget);
        return true;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "Liquid Glass GPU (alpha): %s\n", error.what());
        reported = true;
        return false;
    }
}
