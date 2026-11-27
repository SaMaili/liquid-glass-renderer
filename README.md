# Liquid Glass Renderer

An independent offscreen renderer for Linux with a Rust library, a Python package, and shared GLSL shaders. The host application supplies backdrop pixels, layers, and material parameters. The renderer refracts and filters the backdrop through the glass shapes and returns a composited image.

The renderer was built by reverse-engineering a macOS system image and uses the original material and animation values recovered for the rendering paths that have been verified.

## Renderer in action

### Glass morphing

![Two glass shapes stretch, merge into one surface, and separate again](media/morphing.gif)

### Multilingual hello

![A glass Apple logo morphs into hello, followed by localized greetings and Welcome over the Golden Gate Bridge](media/hello_dynamic.gif)

A glass Apple logo morphs into “hello,” followed by all 49 localized Hello variants bundled with macOS 27.2 and the English “Welcome.” The preview uses the Setup Assistant’s Macintosh Skeletal lettering and recovered optical parameters. The Golden Gate scene, logo transition, and pacing of the language rotation are specific to this preview.

### Elastic drag and release

![A pointer stretches a glass control, then releases it so it springs back into shape](media/elastic-drag.gif)

The demo magnifies the deformation and slows the release so the spring motion is easy to see.

### Quickshell widgets

Calendar and clock widgets from the live Quickshell integration on Hyprland. All previews use the Golden Gate backdrop.

| Calendar | Digital clock |
| --- | --- |
| ![Quickshell calendar refracting the Golden Gate Bridge](media/quickshell-calendar.webp) | ![Quickshell digital clock over the rocky Golden Gate shoreline](media/quickshell-clock.webp) |

## Choosing an entry point

You do not need to read any reverse-engineering internals to build with this renderer:

1. **Standalone apps & creative tools (e.g. an Icon Composer, Rust / C++ / Python):**
   Use the `liquid_glass_core` Rust crate (`Renderer`, `Frame`, `SdrRecipe`, `RecipeRole::AppIcons`), the Python package (`SceneRenderer`, `Element`, `Material`), or the C ABI in [`integrations/hyprland/widget_renderer.hpp`](integrations/hyprland/widget_renderer.hpp) (`lg_widget_prepare` for real-time GPU viewports, `lg_widget_render` for RGBA/PNG export). No compositor integration required when rendering glass over your own app canvas.
2. **Desktop shells & Wayland overlays (Hyprland + Quickshell / GTK4-Layer-Shell):**
   Use the compositor GPU plugin in [`integrations/hyprland/`](integrations/hyprland/) and see **[`integrations/hyprland/QUICKSTART.md`](integrations/hyprland/QUICKSTART.md)** for the complete IPC reference (material roles `0..7`, `localBounds`, vertical Dual-Tint `grad`, and GPU `sdfUnion` Mitosis).

## Installation and quick start

You need Linux, EGL, and an OpenGL 4.5 driver. Mesa's llvmpipe software renderer also works. For Rust, install a linker and the EGL and GL development libraries, such as `libegl-dev` and `libgl-dev` on Debian or Ubuntu. The Rust build was tested with Rust 1.99. The Python package requires Python 3.10+, NumPy, and Pillow.

### Rust

```sh
cargo build --release --manifest-path rust/glass-core/Cargo.toml
mkdir -p out
cargo run --release --manifest-path rust/glass-core/Cargo.toml --example pass -- out/rust-glass.ppm
```

The example renders two overlapping glass shapes and updates part of the backdrop. Use `Renderer`, `Frame`, `SdrRecipe`, and `LiveScene` from the `liquid_glass_core` crate to integrate the renderer into your application.

### Python

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install .
python -m liquid_glass --output out/python-glass.png
```

Build custom scenes with `SceneRenderer`, `Element`, and `Material`. The Python package takes explicit material values. Images are arrays of shape `(height, width, 4)`. The compositor defaults to linear, premultiplied RGBA.

## Feature checklist

- [x] Offscreen EGL/OpenGL rendering with shared GLSL passes
- [x] Distance fields from geometry or rasters, with smooth unions of multiple shapes
- [x] Refraction, chromatic aberration, blur mip pyramids, and backdrop composition
- [x] Edge highlights, edge color bleed, and shadows in the corresponding material passes
- [x] Ordered glass groups that sample either the original backdrop or the composited result
- [x] Spring animations that can be retargeted while in motion
- [x] Rust SDR material recipes for Clear, Regular, Dock, Popover, and AppIcons with diffusion and tint
- [x] Native vertical Dual-Tint gradient (`widgetTint` → `widgetTint2` with `clearBottomFactor` for dark-top / crystal-clear-bottom glass)
- [x] GPU `sdfUnion` Mitosis with anisotropic ellipses (smooth button/droplet merging)
- [x] `26B123` `SiriEffect` GLSL port (`shaders/siri-effect.frag`: 4× RGSS MSAA chromatic wave + 6-pair fluid metaball dots)
- [x] Hyprland GPU compositor plugin (`integrations/hyprland/`) with C ABI and IPC quickstart
- [x] Rust interaction effects for hover, press, and localized glow
- [x] Rust partial backdrop updates and GPU resource reuse
- [ ] General automatic material and shape matching as in the macOS compositor
- [ ] Full visual parity with Apple's native output across graphics drivers

The repository includes runnable renderers and synthetic examples.
