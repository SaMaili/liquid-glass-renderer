#pragma once

#include <GLES3/gl32.h>

struct FramebufferRegion {
    int left, top, right, bottom;
};

// Hyprland's intermediate framebuffer uses monitor-local row coordinates.
// The SDF raster starts with row zero of the copied region as well.
inline void copyToWidgetTexture(FramebufferRegion crop) {
    glBlitFramebuffer(crop.left, crop.top, crop.right, crop.bottom,
                      0, 0, crop.right - crop.left, crop.bottom - crop.top,
                      GL_COLOR_BUFFER_BIT, GL_NEAREST);
}

inline void copyToCompositor(FramebufferRegion crop, FramebufferRegion widget) {
    glBlitFramebuffer(widget.left - crop.left, widget.top - crop.top,
                      widget.right - crop.left, widget.bottom - crop.top,
                      widget.left, widget.top, widget.right, widget.bottom,
                      GL_COLOR_BUFFER_BIT, GL_NEAREST);
}
