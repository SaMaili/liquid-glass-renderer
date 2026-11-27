#pragma once

#include <EGL/egl.h>
#include <array>
#include <cstddef>
#include <cstdint>

struct WidgetRenderer;
struct Widget {
    uint32_t width, height;
    float scale;
    std::array<float, 4> bounds;
    float radius, diffusion;
    uint32_t role, appearance;
    float opacity;
    std::array<float, 4> tint;
    float pointer = 0.5F;
    std::array<float, 4> tint2 = {0.F, 0.F, 0.F, -1.F};
    std::array<float, 2> tintStops = {0.F, 1.F};
};
extern "C" WidgetRenderer* lg_widget_renderer_create();
extern "C" void lg_widget_renderer_destroy(WidgetRenderer*);
extern "C" bool lg_widget_render(WidgetRenderer*, const Widget*, uint8_t*, size_t);
struct PreparedWidget {
    float geometry[6][4];
    float transform[4][4];
    uint8_t uniforms[320], extension[32];
    uint32_t levels;
    float widgetPlate[3][4];
    float widgetTint[4];
    float widgetTint2[4];
    float widgetTintAxis[4];
    float legibilityLod;
    float highlightMatrix[4][4];
    float highlightLobes[2][4];
};
extern "C" bool lg_widget_prepare(const Widget*, PreparedWidget*);

class EglBinding {
    EGLDisplay display = eglGetCurrentDisplay();
    EGLContext context = eglGetCurrentContext();
    EGLSurface draw = eglGetCurrentSurface(EGL_DRAW);
    EGLSurface read = eglGetCurrentSurface(EGL_READ);
    EGLenum api = eglQueryAPI();

  public:
    bool restore() const {
        if (display == EGL_NO_DISPLAY) {
            const EGLDisplay current = eglGetCurrentDisplay();
            if (current != EGL_NO_DISPLAY)
                eglMakeCurrent(current, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
            return eglBindAPI(api);
        }
        return eglBindAPI(api) && eglMakeCurrent(display, draw, read, context);
    }
};
