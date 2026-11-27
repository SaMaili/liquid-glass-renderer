// Compositor pass for dedicated Quickshell widget surfaces on Hyprland 0.57.
#include "gpu_widget.hpp"
#include <GLES3/gl32.h>
#include <hyprland/src/desktop/view/LayerSurface.hpp>
#include <hyprland/src/plugins/HookSystem.hpp>
#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>
#include <hyprland/src/render/gl/GLFramebuffer.hpp>

#include <algorithm>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <stdexcept>
#include <sstream>
#include <string_view>
#include <vector>

namespace {
HANDLE pluginHandle;
CFunctionHook* layerHook;
std::unique_ptr<GpuWidgets> widgetRenderer;
SP<IPC::Socket1::SCommand> statusCommand;
SP<IPC::Socket1::SCommand> styleCommand;
SP<IPC::Socket1::SCommand> iconsCommand;
uint64_t rendered = 0, failed = 0;
uint64_t renderedDock = 0, renderedPopover = 0, renderedLabels = 0, renderedShell = 0, renderedIcons = 0;
uint64_t drawTotalUs = 0;
SP<Render::IFramebuffer> sharedMaskFB;
constexpr int padding = 96;

struct Material {
    uint32_t role = 2, appearance = 1;
    float radius = 30.F, diffusion = 0.5F;
    std::array<float, 4> tint{};
    float pointer = 0.5F;
    std::array<float, 4> tint2{0.F, 0.F, 0.F, -1.F};
    std::array<float, 2> tintStops{0.F, 1.F};
    bool operator==(const Material&) const = default;
};

struct IconPlate {
    double x = 0, y = 0, w = 0, h = 0;
    float radius = 12.F;
    bool operator==(const IconPlate&) const = default;
};

struct WidgetStyle {
    Material material;
    PHLLSREF layer;
    bool configured = false;
    std::optional<CBox> localBounds;
    Material iconMaterial{7, 1, 12.F, 0.5F, {0.F, 0.F, 0.F, 0.F}, 0.5F};
    std::vector<IconPlate> icons;
};
std::array<WidgetStyle, 1024> styles;
constexpr std::string_view widgetNamespace = "liquidglass-rust-widget~";

std::optional<size_t> widgetId(std::string_view name) {
    if (!name.starts_with(widgetNamespace))
        return std::nullopt;
    name.remove_prefix(widgetNamespace.size());
    unsigned id;
    auto result = std::from_chars(name.data(), name.data() + name.size(), id);
    if (result.ec != std::errc{} || result.ptr != name.data() + name.size() || id >= styles.size())
        return std::nullopt;
    return id;
}

IPC::Socket1::SResponse editStyle(const IPC::Socket1::SRequest& request) {
    std::istringstream input(request.command);
    std::string command;
    size_t id;
    if (!(input >> command >> id) || command != "liquid-glass-style" || id >= styles.size())
        return "invalid widget id";
    auto& entry = styles[id];
    input >> std::ws;
    if (!input.eof()) {
        if (input.peek() == 'o') {
            std::string action;
            input >> action >> std::ws;
            if (action != "off" || !input.eof() || id < 768)
                return "invalid surface action";
            entry.configured = false;
            entry.localBounds.reset();
            entry.icons.clear();
            if (const auto layer = entry.layer.lock(); layer && g_pHyprRenderer)
                if (const auto monitor = layer->m_monitor.lock())
                    g_pHyprRenderer->damageMonitor(monitor);
            return "ok\n";
        }
        int role, appearance, radius, diffusion, red, green, blue, strength;
        if (!(input >> role >> appearance >> radius >> diffusion >> red >> green >> blue >> strength)
            || role < 0 || role > 7 || appearance < 0 || appearance > 1
            || radius < 0 || radius > 1024 || diffusion < 0 || diffusion > 100
            || std::min({red, green, blue}) < 0 || std::max({red, green, blue}) > 255
            || strength < 0 || strength > 100)
            return "invalid widget material";
        std::optional<CBox> local;
        if (role == 3) {
            double x, y, width, height;
            if (!(input >> x >> y >> width >> height)
                || !std::isfinite(x) || !std::isfinite(y)
                || !std::isfinite(width) || !std::isfinite(height)
                || std::abs(x) > 8192 || std::abs(y) > 8192
                || width <= 0 || width > 2048 || height <= 0 || height > 512)
                return "invalid dock bounds";
            local = CBox{x, y, width, height};
        } else if (role == 0 || role == 1 || role == 4) {
            input >> std::ws;
            if (!input.eof() && input.peek() != 'g') {
                double x, y, width, height;
                if (!(input >> x >> y >> width >> height)
                    || !std::isfinite(x) || !std::isfinite(y)
                    || !std::isfinite(width) || !std::isfinite(height)
                    || std::abs(x) > 8192 || std::abs(y) > 8192
                    || width <= 0 || width > 2048 || height <= 0 || height > 2048)
                    return "invalid panel bounds";
                local = CBox{x, y, width, height};
            }
        }
        float pointer = role == 6 ? 0.55F : 0.5F;
        if (role == 5 && (!(input >> pointer) || !std::isfinite(pointer) || pointer < 0.F || pointer > 1.F))
            return "invalid label pointer";
        if (role == 6) {
            input >> std::ws;
            if (!input.eof() && (!(input >> pointer) || !std::isfinite(pointer) || pointer < 0.1F || pointer > 0.95F))
                return "invalid shell pedestal";
        }
        std::array<float, 4> tint2{0.F, 0.F, 0.F, -1.F};
        std::array<float, 2> tintStops{0.F, 1.F};
        input >> std::ws;
        if (!input.eof() && input.peek() == 'g') {
            std::string kw;
            int r2, g2, b2, s2, startPct, endPct;
            if (!(input >> kw >> r2 >> g2 >> b2 >> s2 >> startPct >> endPct)
                || kw != "grad"
                || std::min({r2, g2, b2}) < 0 || std::max({r2, g2, b2}) > 255
                || s2 < 0 || s2 > 100
                || startPct < 0 || startPct > 100 || endPct < 0 || endPct > 100)
                return "invalid gradient tint";
            tint2 = {r2 / 255.F, g2 / 255.F, b2 / 255.F, s2 / 100.F};
            tintStops = {startPct / 100.F, endPct / 100.F};
        }
        input >> std::ws;
        if (!input.eof())
            return "unexpected widget material values";
        const Material next{static_cast<uint32_t>(role), static_cast<uint32_t>(appearance),
            static_cast<float>(radius), diffusion / 100.F,
            {red / 255.F, green / 255.F, blue / 255.F, strength / 100.F}, pointer, tint2, tintStops};
        if (!entry.configured || entry.material != next || entry.localBounds != local) {
            const auto previous = entry.localBounds;
            entry.material = next;
            entry.localBounds = local;
            entry.configured = true;
            if (const auto layer = entry.layer.lock(); layer && g_pHyprRenderer) {
                const auto origin = layer->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
                const auto damage = [&](CBox bounds) {
                    bounds.translate(origin).expand(padding);
                    g_pHyprRenderer->damageBox(bounds);
                };
                if (local) {
                    if (previous && previous != local) damage(*previous);
                    damage(*local);
                } else {
                    damage(CBox{Vector2D{}, layer->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT)});
                }
            }
        }
    }
    const auto& m = entry.material;
    const auto& rect = entry.localBounds;
    const auto bounds = rect ? std::format("[{},{},{},{}]", rect->x, rect->y, rect->w, rect->h) : "null";
    return IPC::Socket1::SResponse(std::format(
        R"({{"configured":{},"role":{},"appearance":{},"radius":{},"legibility":{},"tint":[{},{},{},{}],"bounds":{}}})" "\n",
        entry.configured, m.role, m.appearance, m.radius, m.diffusion, m.tint[0], m.tint[1], m.tint[2], m.tint[3], bounds));
}

IPC::Socket1::SResponse editIcons(const IPC::Socket1::SRequest& request) {
    std::istringstream input(request.command);
    std::string command;
    size_t id;
    if (!(input >> command >> id) || command != "liquid-glass-icons" || id >= styles.size())
        return "invalid widget id";
    auto& entry = styles[id];
    input >> std::ws;
    if (input.eof())
        return IPC::Socket1::SResponse(std::format(R"({{"count":{}}})" "\n", entry.icons.size()));

    auto damageIcons = [&](const std::vector<IconPlate>& list) {
        if (list.empty())
            return;
        if (const auto layer = entry.layer.lock(); layer && g_pHyprRenderer) {
            const auto origin = layer->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
            double minX = list[0].x, minY = list[0].y;
            double maxX = list[0].x + list[0].w, maxY = list[0].y + list[0].h;
            for (const auto& icon : list) {
                minX = std::min(minX, icon.x);
                minY = std::min(minY, icon.y);
                maxX = std::max(maxX, icon.x + icon.w);
                maxY = std::max(maxY, icon.y + icon.h);
            }
            CBox box{minX, minY, maxX - minX, maxY - minY};
            box.translate(origin).expand(32);
            g_pHyprRenderer->damageBox(box);
        }
    };

    if (input.peek() == 'o') {
        std::string action;
        input >> action >> std::ws;
        if (action != "off" || !input.eof())
            return "invalid icons action";
        if (!entry.icons.empty()) {
            damageIcons(entry.icons);
            entry.icons.clear();
        }
        return "ok\n";
    }

    int appearance, diffusion, red, green, blue, strength;
    if (!(input >> appearance >> diffusion >> red >> green >> blue >> strength)
        || appearance < 0 || appearance > 1 || diffusion < 0 || diffusion > 100
        || std::min({red, green, blue}) < 0 || std::max({red, green, blue}) > 255
        || strength < 0 || strength > 100)
        return "invalid icon material";

    std::vector<IconPlate> nextIcons;
    nextIcons.reserve(32);
    double x, y, w, h;
    float r;
    while (input >> x >> y >> w >> h >> r) {
        if (nextIcons.size() >= 64
            || !std::isfinite(x) || !std::isfinite(y) || !std::isfinite(w) || !std::isfinite(h)
            || !std::isfinite(r) || std::abs(x) > 8192 || std::abs(y) > 8192
            || w < 4 || w > 512 || h < 4 || h > 512 || r < 0 || r > 256)
            return "invalid icon rect";
        nextIcons.push_back({x, y, w, h, r});
    }
    if (!input.eof())
        return "invalid icon list";

    const uint32_t iconRole = (entry.material.role == 0 || entry.material.role == 1 || entry.material.role == 4) ? entry.material.role : 7;
    const Material nextMat{iconRole, static_cast<uint32_t>(appearance), 12.F, diffusion / 100.F,
        {red / 255.F, green / 255.F, blue / 255.F, strength / 100.F}, 0.5F};
    if (entry.iconMaterial != nextMat || entry.icons != nextIcons) {
        damageIcons(entry.icons);
        entry.iconMaterial = nextMat;
        entry.icons = std::move(nextIcons);
        damageIcons(entry.icons);
    }
    return "ok\n";
}

std::optional<Material> materialFor(std::string_view name) {
    const auto separator = name.find('~');
    const auto base = name.substr(0, separator);
    Material material;
    if (base == "liquidglass-rust-regular-dark") material = {0, 1};
    else if (base == "liquidglass-rust-regular-light") material = {0, 0};
    else if (base == "liquidglass-rust-clear-dark") material = {1, 1};
    else if (base == "liquidglass-rust-clear-light") material = {1, 0};
    else return std::nullopt;
    if (separator == std::string_view::npos)
        return material;
    const auto values = name.substr(separator + 1);
    const auto delimiter = values.find('~');
    if (delimiter == std::string_view::npos)
        return std::nullopt;
    const auto parse = [](std::string_view source, int& value) {
        const auto end = source.data() + source.size();
        const auto result = std::from_chars(source.data(), end, value);
        return result.ec == std::errc{} && result.ptr == end;
    };
    int radius, diffusion;
    if (!parse(values.substr(0, delimiter), radius) || !parse(values.substr(delimiter + 1), diffusion)
        || radius < 0 || radius > 4096 || diffusion < 0 || diffusion > 100)
        return std::nullopt;
    material.radius = radius;
    material.diffusion = diffusion / 100.F;
    return material;
}

std::optional<CBox> widgetBox(const PHLLS& layer, const PHLMONITOR& monitor, const std::optional<CBox>& local) {
    if (!layer || !monitor)
        return std::nullopt;
    auto box = CBox{layer->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT),
                    layer->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT)};
    if (local) {
        box.x += local->x;
        box.y += local->y;
        box.w = local->w;
        box.h = local->h;
    }
    box.translate(-monitor->m_position);
    if (!std::isfinite(box.x) || !std::isfinite(box.y) || !std::isfinite(box.w)
        || !std::isfinite(box.h) || box.w < 2 || box.h < 2)
        return std::nullopt;
    return box;
}

class WidgetPass final : public IPassElement {
    PHLLSREF layer;
    Material material;
    std::optional<CBox> localBounds;
    std::vector<IconPlate> mitosisIcons;

  public:
    WidgetPass(const PHLLS& surface, Material recipe, std::optional<CBox> local = {},
               std::vector<IconPlate> mitosis = {})
        : layer(surface), material(recipe), localBounds(local), mitosisIcons(std::move(mitosis)) {}
    const char* passName() override { return "LiquidGlassWidget"; }
    ePassElementType type() override { return EK_CUSTOM; }
    bool needsLiveBlur(Render::CRenderContext& ctx) override {
        return widgetBox(layer.lock(), ctx.m_data.pMonitor.lock(), localBounds).has_value();
    }
    bool needsPrecomputeBlur(Render::CRenderContext&) override { return false; }
    bool disableSimplification(Render::CRenderContext&) override { return true; }

    std::optional<CBox> combinedLocal() const {
        if (!localBounds)
            return std::nullopt;
        double minX = localBounds->x, minY = localBounds->y;
        double maxX = localBounds->x + localBounds->w, maxY = localBounds->y + localBounds->h;
        for (const auto& icon : mitosisIcons) {
            minX = std::min(minX, icon.x);
            minY = std::min(minY, icon.y);
            maxX = std::max(maxX, icon.x + icon.w);
            maxY = std::max(maxY, icon.y + icon.h);
        }
        return CBox{minX, minY, maxX - minX, maxY - minY};
    }

    std::optional<CBox> boundingBox(Render::CRenderContext& ctx) override {
        const auto monitor = ctx.m_data.pMonitor.lock();
        auto box = widgetBox(layer.lock(), monitor, combinedLocal());
        if (box) {
            box->expand(padding / monitor->m_scale);
            return box;
        }
        return CBox{0, 0, 1, 1};
    }

    std::vector<UP<IPassElement>> draw(Render::CRenderContext& ctx) override {
        const auto surface = layer.lock();
        const auto monitor = ctx.m_data.pMonitor.lock();
        auto bodyBox = widgetBox(surface, monitor, localBounds);
        auto unionBox = mitosisIcons.empty() ? bodyBox : widgetBox(surface, monitor, combinedLocal());
        if (!bodyBox || !unionBox || monitor->m_transform != WL_OUTPUT_TRANSFORM_NORMAL || monitor->useFP16()
            || !widgetRenderer || !ctx.m_data.currentFB)
            return {};
        const auto fb = dynamic_cast<Render::GL::CGLFramebuffer*>(ctx.m_data.currentFB.get());
        if (!fb || eglGetCurrentContext() == EGL_NO_CONTEXT)
            return {};
        const int targetW = static_cast<int>(fb->m_size.x);
        const int targetH = static_cast<int>(fb->m_size.y);
        if (targetW != static_cast<int>(monitor->m_transformedSize.x)
            || targetH != static_cast<int>(monitor->m_transformedSize.y))
            return {};

        const float scale = monitor->m_scale;
        bodyBox->scale(scale);
        unionBox->scale(scale);
        const int left = std::clamp(static_cast<int>(std::floor(unionBox->x)), 0, targetW);
        const int top = std::clamp(static_cast<int>(std::floor(unionBox->y)), 0, targetH);
        const int right = std::clamp(static_cast<int>(std::ceil(unionBox->x + unionBox->w)), 0, targetW);
        const int bottom = std::clamp(static_cast<int>(std::ceil(unionBox->y + unionBox->h)), 0, targetH);
        if (left >= right || top >= bottom)
            return {};
        const int pad = (material.role == 3 || material.role == 5 || localBounds.has_value()) ? 48 : padding;
        int cropX = std::max(0, left - pad);
        int cropY = std::max(0, top - pad);
        int cropRight = std::min(targetW, right + pad);
        int cropBottom = std::min(targetH, bottom + pad);
        // Quantize animated Dock, Spotlight & Tooltip crop dimensions to fixed buckets so continuous
        // magnification or panel size changes never reallocate GPU mipchains.
        if (material.role == 3 || localBounds.has_value()) {
            cropX &= ~255;
            cropRight = std::min(targetW, (cropRight + 255) & ~255);
            cropY &= ~63;
            cropBottom = std::min(targetH, (cropBottom + 63) & ~63);
        } else if (material.role == 5) {
            cropX &= ~63;
            cropRight = std::min(targetW, (cropRight + 63) & ~63);
            cropY &= ~63;
            cropBottom = std::min(targetH, (cropBottom + 63) & ~63);
        }
        const int width = cropRight - cropX;
        const int height = cropBottom - cropY;
        if (width > 4096 || height > 4096 || width * height > 2600000)
            return {};

        const FramebufferRegion crop{cropX, cropY, cropRight, cropBottom};
        const Widget renderParams{
            .width = static_cast<uint32_t>(width), .height = static_cast<uint32_t>(height),
            .scale = scale,
            .bounds = {static_cast<float>(bodyBox->x - cropX), static_cast<float>(bodyBox->y - cropY),
                       static_cast<float>(bodyBox->w), static_cast<float>(bodyBox->h)},
            .radius = material.radius, .diffusion = material.diffusion,
            .role = material.role, .appearance = material.appearance,
            .opacity = std::clamp(surface->alpha().getTotal(), 0.F, 1.F),
            .tint = material.tint,
            .pointer = material.pointer,
            .tint2 = material.tint2,
            .tintStops = material.tintStops,
        };
        MitosisShapes mitosis{};
        if (!mitosisIcons.empty()) {
            // 10 pt gap at rest => d=5 pt at midpoint; k=14 gives smin=+1.5 pt (crisp separation at rest)
            // and melts into a smooth liquid bridge as soon as gap < 7 pt during Mitosis expansion!
            mitosis.smoothness = 14.0F;
            const size_t n = std::min<size_t>(mitosisIcons.size(), 4);
            for (size_t i = 0; i < n; ++i) {
                const auto& icon = mitosisIcons[i];
                if (auto ib = widgetBox(surface, monitor, CBox{icon.x, icon.y, icon.w, icon.h})) {
                    ib->scale(scale);
                    const float cx = (static_cast<float>(ib->x + ib->w * 0.5) - static_cast<float>(cropX)) / scale;
                    const float cy = (static_cast<float>(ib->y + ib->h * 0.5) - static_cast<float>(cropY)) / scale;
                    const float rx = static_cast<float>(ib->w * 0.5) / scale;
                    const float ry = static_cast<float>(ib->h * 0.5) / scale;
                    mitosis.circles[mitosis.count++] = {cx, cy, rx, ry};
                }
            }
        }
        const auto t0 = std::chrono::steady_clock::now();
        if (widgetRenderer->draw(fb->getFBID(), crop, {left, top, right, bottom}, renderParams, mitosis)) {
            drawTotalUs += std::chrono::duration_cast<std::chrono::microseconds>(
                std::chrono::steady_clock::now() - t0).count();
            ++rendered;
            renderedDock += material.role == 3;
            renderedPopover += material.role == 4;
            renderedLabels += material.role == 5;
            renderedIcons += material.role == 7;
        } else
            ++failed;
        return {};
    }
};

class DockIconsPass final : public IPassElement {
    PHLLSREF layer;
    Material material;
    std::vector<IconPlate> icons;

  public:
    DockIconsPass(const PHLLS& surface, Material recipe, std::vector<IconPlate> plates)
        : layer(surface), material(recipe), icons(std::move(plates)) {}
    const char* passName() override { return "LiquidGlassDockIcons"; }
    ePassElementType type() override { return EK_CUSTOM; }
    bool needsLiveBlur(Render::CRenderContext&) override { return !icons.empty(); }
    bool needsPrecomputeBlur(Render::CRenderContext&) override { return false; }
    bool disableSimplification(Render::CRenderContext&) override { return true; }

    std::optional<CBox> boundingBox(Render::CRenderContext& ctx) override {
        const auto surface = layer.lock();
        const auto monitor = ctx.m_data.pMonitor.lock();
        if (!surface || !monitor || icons.empty())
            return CBox{0, 0, 1, 1};
        double minX = icons[0].x, minY = icons[0].y;
        double maxX = icons[0].x + icons[0].w, maxY = icons[0].y + icons[0].h;
        for (const auto& icon : icons) {
            minX = std::min(minX, icon.x);
            minY = std::min(minY, icon.y);
            maxX = std::max(maxX, icon.x + icon.w);
            maxY = std::max(maxY, icon.y + icon.h);
        }
        auto box = widgetBox(surface, monitor, CBox{minX, minY, maxX - minX, maxY - minY});
        if (box) {
            box->expand(24.0 / monitor->m_scale);
            return box;
        }
        return CBox{0, 0, 1, 1};
    }

    std::vector<UP<IPassElement>> draw(Render::CRenderContext& ctx) override {
        const auto surface = layer.lock();
        const auto monitor = ctx.m_data.pMonitor.lock();
        if (!surface || !monitor || icons.empty()
            || monitor->m_transform != WL_OUTPUT_TRANSFORM_NORMAL || monitor->useFP16()
            || !widgetRenderer || !ctx.m_data.currentFB)
            return {};
        const auto fb = dynamic_cast<Render::GL::CGLFramebuffer*>(ctx.m_data.currentFB.get());
        if (!fb || eglGetCurrentContext() == EGL_NO_CONTEXT)
            return {};
        const int targetW = static_cast<int>(fb->m_size.x);
        const int targetH = static_cast<int>(fb->m_size.y);
        if (targetW != static_cast<int>(monitor->m_transformedSize.x)
            || targetH != static_cast<int>(monitor->m_transformedSize.y))
            return {};

        const float scale = monitor->m_scale;
        const float opacity = std::clamp(surface->alpha().getTotal(), 0.F, 1.F);
        constexpr int iconPad = 24;
        for (const auto& icon : icons) {
            auto box = widgetBox(surface, monitor, CBox{icon.x, icon.y, icon.w, icon.h});
            if (!box)
                continue;
            box->scale(scale);
            const int left = std::clamp(static_cast<int>(std::floor(box->x)), 0, targetW);
            const int top = std::clamp(static_cast<int>(std::floor(box->y)), 0, targetH);
            const int right = std::clamp(static_cast<int>(std::ceil(box->x + box->w)), 0, targetW);
            const int bottom = std::clamp(static_cast<int>(std::ceil(box->y + box->h)), 0, targetH);
            if (left >= right || top >= bottom)
                continue;
            int cropX = std::max(0, left - iconPad);
            int cropY = std::max(0, top - iconPad);
            int cropRight = std::min(targetW, right + iconPad);
            int cropBottom = std::min(targetH, bottom + iconPad);
            // Quantize to 64px buckets so all icons share the same cached GPU FBO.
            const int width = std::min(targetW - cropX, ((cropRight - cropX) + 63) & ~63);
            const int height = std::min(targetH - cropY, ((cropBottom - cropY) + 63) & ~63);
            cropRight = cropX + width;
            cropBottom = cropY + height;
            if (width <= 0 || height <= 0 || width > 1024 || height > 1024)
                continue;
            const FramebufferRegion crop{cropX, cropY, cropRight, cropBottom};
            const Widget renderParams{
                .width = static_cast<uint32_t>(width), .height = static_cast<uint32_t>(height),
                .scale = scale,
                .bounds = {static_cast<float>(box->x - cropX), static_cast<float>(box->y - cropY),
                           static_cast<float>(box->w), static_cast<float>(box->h)},
                .radius = icon.radius, .diffusion = material.diffusion,
                .role = material.role, .appearance = material.appearance,
                .opacity = opacity,
                .tint = material.tint,
                .pointer = 0.5F,
            };
            const auto t0 = std::chrono::steady_clock::now();
            if (widgetRenderer->draw(fb->getFBID(), crop, {left, top, right, bottom}, renderParams)) {
                drawTotalUs += std::chrono::duration_cast<std::chrono::microseconds>(
                    std::chrono::steady_clock::now() - t0).count();
                ++rendered;
                ++renderedIcons;
            } else {
                ++failed;
            }
        }
        return {};
    }
};

struct MaskCaptureState {
    SP<Render::IFramebuffer> savedFB;
    CRegion savedDamage;
    bool active = false;
};

class MaskCaptureBeginPass final : public IPassElement {
    PHLLSREF layer;
    SP<MaskCaptureState> state;

  public:
    MaskCaptureBeginPass(const PHLLS& surface, SP<MaskCaptureState> capture)
        : layer(surface), state(std::move(capture)) {}
    const char* passName() override { return "LiquidGlassMaskBegin"; }
    ePassElementType type() override { return EK_CUSTOM; }
    bool needsLiveBlur(Render::CRenderContext& ctx) override {
        return widgetBox(layer.lock(), ctx.m_data.pMonitor.lock(), std::nullopt).has_value();
    }
    bool needsPrecomputeBlur(Render::CRenderContext&) override { return false; }
    bool disableSimplification(Render::CRenderContext&) override { return true; }

    std::optional<CBox> boundingBox(Render::CRenderContext& ctx) override {
        const auto monitor = ctx.m_data.pMonitor.lock();
        if (auto box = widgetBox(layer.lock(), monitor, std::nullopt))
            return box;
        return CBox{0, 0, 1, 1};
    }

    std::vector<UP<IPassElement>> draw(Render::CRenderContext& ctx) override {
        const auto surface = layer.lock();
        const auto monitor = ctx.m_data.pMonitor.lock();
        auto box = widgetBox(surface, monitor, std::nullopt);
        if (!box || monitor->m_transform != WL_OUTPUT_TRANSFORM_NORMAL || monitor->useFP16()
            || !widgetRenderer || !ctx.m_data.currentFB || !g_pHyprRenderer)
            return {};
        const auto fb = dynamic_cast<Render::GL::CGLFramebuffer*>(ctx.m_data.currentFB.get());
        if (!fb || eglGetCurrentContext() == EGL_NO_CONTEXT)
            return {};
        const int targetW = static_cast<int>(fb->m_size.x);
        const int targetH = static_cast<int>(fb->m_size.y);
        if (targetW != static_cast<int>(monitor->m_transformedSize.x)
            || targetH != static_cast<int>(monitor->m_transformedSize.y)
            || targetW > 4096 || targetH > 4096 || targetW * targetH > 9000000)
            return {};

        if (!sharedMaskFB)
            sharedMaskFB = g_pHyprRenderer->createFB("LiquidGlassMask");
        if (!sharedMaskFB->isAllocated()
            || static_cast<int>(sharedMaskFB->m_size.x) != targetW
            || static_cast<int>(sharedMaskFB->m_size.y) != targetH) {
            if (!sharedMaskFB->alloc(targetW, targetH, DRM_FORMAT_ARGB8888))
                return {};
        }

        state->savedFB = ctx.m_data.currentFB;
        state->savedDamage = ctx.m_data.damage;
        g_pHyprRenderer->bindFB(ctx, sharedMaskFB);
        const GLboolean scissorWas = glIsEnabled(GL_SCISSOR_TEST);
        glDisable(GL_SCISSOR_TEST);
        glClearColor(0.F, 0.F, 0.F, 0.F);
        glClear(GL_COLOR_BUFFER_BIT);
        if (scissorWas)
            glEnable(GL_SCISSOR_TEST);
        ctx.m_data.damage = CRegion{0, 0, targetW, targetH};
        state->active = true;
        return {};
    }
};

class MaskCaptureEndPass final : public IPassElement {
    PHLLSREF layer;
    Material material;
    SP<MaskCaptureState> state;

  public:
    MaskCaptureEndPass(const PHLLS& surface, Material recipe, SP<MaskCaptureState> capture)
        : layer(surface), material(recipe), state(std::move(capture)) {}
    const char* passName() override { return "LiquidGlassMaskEnd"; }
    ePassElementType type() override { return EK_CUSTOM; }
    bool needsLiveBlur(Render::CRenderContext&) override { return true; }
    bool needsPrecomputeBlur(Render::CRenderContext&) override { return false; }
    bool disableSimplification(Render::CRenderContext&) override { return true; }

    std::optional<CBox> boundingBox(Render::CRenderContext& ctx) override {
        const auto monitor = ctx.m_data.pMonitor.lock();
        if (auto box = widgetBox(layer.lock(), monitor, std::nullopt))
            return box;
        return CBox{0, 0, 1, 1};
    }

    std::vector<UP<IPassElement>> draw(Render::CRenderContext& ctx) override {
        if (!state->active || !state->savedFB || !g_pHyprRenderer)
            return {};
        auto savedFB = std::move(state->savedFB);
        state->active = false;
        g_pHyprRenderer->bindFB(ctx, savedFB);
        ctx.m_data.damage = state->savedDamage;

        const auto surface = layer.lock();
        const auto monitor = ctx.m_data.pMonitor.lock();
        auto box = widgetBox(surface, monitor, std::nullopt);
        const auto fb = dynamic_cast<Render::GL::CGLFramebuffer*>(savedFB.get());
        const auto maskTex = sharedMaskFB ? sharedMaskFB->getTexture() : nullptr;
        if (!box || !fb || !maskTex || maskTex->m_texID == 0 || !widgetRenderer)
            return {};

        const int targetW = static_cast<int>(fb->m_size.x);
        const int targetH = static_cast<int>(fb->m_size.y);
        const float scale = monitor->m_scale;
        box->scale(scale);
        const int left = std::clamp(static_cast<int>(std::floor(box->x)), 0, targetW);
        const int top = std::clamp(static_cast<int>(std::floor(box->y)), 0, targetH);
        const int right = std::clamp(static_cast<int>(std::ceil(box->x + box->w)), 0, targetW);
        const int bottom = std::clamp(static_cast<int>(std::ceil(box->y + box->h)), 0, targetH);
        if (left >= right || top >= bottom)
            return {};

        const int cropX = std::max(0, left - padding);
        const int cropY = std::max(0, top - padding);
        const int cropRight = std::min(targetW, right + padding);
        const int cropBottom = std::min(targetH, bottom + padding);
        const int width = cropRight - cropX;
        const int height = cropBottom - cropY;
        if (width > 4096 || height > 4096 || width * height > 9000000)
            return {};

        const FramebufferRegion crop{cropX, cropY, cropRight, cropBottom};
        const Widget renderParams{
            .width = static_cast<uint32_t>(width), .height = static_cast<uint32_t>(height),
            .scale = scale,
            .bounds = {static_cast<float>(box->x - cropX), static_cast<float>(box->y - cropY),
                       static_cast<float>(box->w), static_cast<float>(box->h)},
            .radius = material.radius, .diffusion = material.diffusion,
            .role = material.role, .appearance = material.appearance,
            .opacity = std::clamp(surface->alpha().getTotal(), 0.F, 1.F),
            .tint = material.tint,
            .pointer = material.pointer,
            .tint2 = material.tint2,
            .tintStops = material.tintStops,
        };
        const auto t0 = std::chrono::steady_clock::now();
        if (widgetRenderer->drawAlphaMask(fb->getFBID(), maskTex->m_texID, material.pointer,
                                          crop, {left, top, right, bottom}, renderParams)) {
            drawTotalUs += std::chrono::duration_cast<std::chrono::microseconds>(
                std::chrono::steady_clock::now() - t0).count();
            ++rendered;
            ++renderedShell;
        } else {
            ++failed;
        }
        return {};
    }
};

using RenderLayer = void (*)(Render::IHyprRenderer*, Render::CRenderContext&, PHLLS, PHLMONITOR,
                             const Time::steady_tp&, bool, bool, bool);

void renderLayer(Render::IHyprRenderer* renderer, Render::CRenderContext& ctx, PHLLS layer,
                 PHLMONITOR monitor, const Time::steady_tp& now, bool popups, bool lockscreen, bool workspaceScene) {
    if (!popups && !lockscreen && !ctx.m_renderingSnapshot && layer && layer->mapped()) {
        if (const auto id = widgetId(layer->m_namespace)) {
            auto& style = styles[*id];
            style.layer = layer;
            if (style.configured && style.material.role == 6
                && widgetBox(layer, monitor, std::nullopt).has_value()) {
                auto capture = makeShared<MaskCaptureState>();
                Render::IHyprRenderer::addPassElement(ctx, makeUnique<MaskCaptureBeginPass>(layer, capture));
                reinterpret_cast<RenderLayer>(layerHook->m_original)(renderer, ctx, layer, monitor, now, popups, lockscreen, workspaceScene);
                Render::IHyprRenderer::addPassElement(ctx, makeUnique<MaskCaptureEndPass>(layer, style.material, capture));
                return;
            }
            const bool useMitosis = style.configured
                && (style.material.role == 0 || style.material.role == 1 || style.material.role == 4)
                && style.localBounds.has_value() && !style.icons.empty();
            if ((style.configured || *id < 768) && widgetBox(layer, monitor, style.localBounds).has_value())
                Render::IHyprRenderer::addPassElement(ctx, makeUnique<WidgetPass>(
                    layer, style.material, style.localBounds, useMitosis ? style.icons : std::vector<IconPlate>{}));
            if (!style.icons.empty() && !useMitosis)
                Render::IHyprRenderer::addPassElement(ctx, makeUnique<DockIconsPass>(layer, style.iconMaterial, style.icons));
        } else if (const auto material = materialFor(layer->m_namespace)) {
            if (widgetBox(layer, monitor, std::nullopt).has_value())
                Render::IHyprRenderer::addPassElement(ctx, makeUnique<WidgetPass>(layer, *material));
        }
    }
    reinterpret_cast<RenderLayer>(layerHook->m_original)(renderer, ctx, layer, monitor, now, popups, lockscreen, workspaceScene);
}
}

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    if (std::string_view(__hyprland_api_get_hash()) != __hyprland_api_get_client_hash())
        throw std::runtime_error("Liquid Glass must be built for the running Hyprland version");
    pluginHandle = handle;
    for (const auto& function : HyprlandAPI::findFunctionsByName(handle, "renderLayer")) {
        if (!function.demangled.contains("IHyprRenderer::renderLayer(Render::CRenderContext&"))
            continue;
        layerHook = HyprlandAPI::createFunctionHook(handle, function.address, reinterpret_cast<void*>(&renderLayer));
        if (layerHook && layerHook->hook()) {
            widgetRenderer = std::make_unique<GpuWidgets>();
            statusCommand = HyprlandAPI::registerHyprCtlCommand(handle, {
                .name = "liquid-glass-status",
                .handler = [](const IPC::Socket1::SRequest&) {
                    return IPC::Socket1::SResponse(std::format(
                        R"({{"rendered":{},"failed":{},"dock":{},"popover":{},"labels":{},"shell":{},"icons":{},"cacheMisses":{},"sdfSkips":{},"avgUs":{}}})",
                        rendered, failed, renderedDock, renderedPopover, renderedLabels, renderedShell, renderedIcons,
                        widgetRenderer ? widgetRenderer->cacheMisses() : 0,
                        widgetRenderer ? widgetRenderer->sdfSkips() : 0,
                        rendered ? drawTotalUs / rendered : 0));
                },
            });
            styleCommand = HyprlandAPI::registerHyprCtlCommand(handle, {
                .name = "liquid-glass-style",
                .match = IPC::Socket1::COMMAND_MATCH_PREFIX,
                .handler = editStyle,
            });
            iconsCommand = HyprlandAPI::registerHyprCtlCommand(handle, {
                .name = "liquid-glass-icons",
                .match = IPC::Socket1::COMMAND_MATCH_PREFIX,
                .handler = editIcons,
            });
            return {"liquid-glass", "Rust Liquid Glass GPU pass for Quickshell surfaces", "Liquid Glass", "0.8"};
        }
        if (layerHook)
            HyprlandAPI::removeFunctionHook(handle, layerHook);
        layerHook = nullptr;
    }
    throw std::runtime_error("Liquid Glass could not attach the Hyprland widget render pass");
}

APICALL EXPORT void PLUGIN_EXIT() {
    if (iconsCommand)
        HyprlandAPI::unregisterHyprCtlCommand(pluginHandle, iconsCommand);
    iconsCommand.reset();
    if (styleCommand)
        HyprlandAPI::unregisterHyprCtlCommand(pluginHandle, styleCommand);
    styleCommand.reset();
    if (statusCommand)
        HyprlandAPI::unregisterHyprCtlCommand(pluginHandle, statusCommand);
    statusCommand.reset();
    if (layerHook)
        HyprlandAPI::removeFunctionHook(pluginHandle, layerHook);
    layerHook = nullptr;
    const EglBinding original;
    if (Render::GL::g_pHyprOpenGL)
        Render::GL::g_pHyprOpenGL->makeEGLCurrent();
    sharedMaskFB.reset();
    widgetRenderer.reset();
    original.restore();
}
