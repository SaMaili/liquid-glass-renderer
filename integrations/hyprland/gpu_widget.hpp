#pragma once

#include "framebuffer_region.hpp"
#include "widget_renderer.hpp"
#include <array>
#include <memory>

struct MitosisShapes {
    int count = 0;
    float smoothness = 0.F;
    // [cx, cy, radius, 0] in logical points relative to crop origin
    std::array<std::array<float, 4>, 4> circles{};
};

class GpuWidgets {
  public:
    GpuWidgets();
    ~GpuWidgets();
    bool draw(GLuint target, FramebufferRegion crop, FramebufferRegion widget, const Widget& params,
              const MitosisShapes& mitosis = {});
    bool drawAlphaMask(GLuint target, GLuint surfaceMaskTex, float pedestal,
                       FramebufferRegion crop, FramebufferRegion widget, const Widget& params);
    uint64_t cacheMisses() const;
    uint64_t sdfSkips() const;

  private:
    struct Resources;
    std::unique_ptr<Resources> resources;
    bool reported = false;
};
