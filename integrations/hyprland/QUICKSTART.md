# Liquid Glass Renderer – Developer Quickstart & API

Dieser Renderer setzt Apples **Liquid Glass** (macOS 26/27, Build `26A434`/`26B123`) als schnellen Rust-/GLSL-GPU-Pass um. Du musst keinen Reverse-Engineering-Code lesen – es gibt **zwei klare Einstiegspunkte**, je nachdem, was du bauen willst:

- **Weg 1: Desktop-Shell / Overlay unter Hyprland** (Quickshell, GTK4-Layer-Shell, beliebige Wayland-Layer-Shell-Apps)
- **Weg 2: Eigenständige App / Creative Tool** (z. B. ein **Icon Composer**, Rust, C++, Qt, GTK4 `GLArea`) – komplett ohne Hyprland und ohne QML!

---

## Material-Rollen (`role`)

| `role` | Name | Verwendung |
| :---: | --- | --- |
| `0` | `regular` | Standard-Liquid-Glass (Panels, Karten, allgemeine Flächen) |
| `1` | `clear` | Klares, stark brechendes Glas (Overlays, Siri-Kugel mit `grad`) |
| `2` | `widget` | Kalibriertes Home-Screen-Widget-Material (mit Legibility-Platte) |
| `3` | `dock` | macOS-Dock-Pille (`DockRecipe.Other`) |
| `4` | `popover` | Popover / Spotlight-Suchleiste (`CartouchePopover`, `regular.size(.large)`) |
| `5` | `tooltip` | Kleiner Tooltip mit Pfeilspitze (`pointer` `0.0..1.0`) |
| `6` | `shell` | Alpha-maskierte Shell-Fläche (beliebige Form via Surface-Maske) |
| `7` | `appIcons` | Apples originales App-Icon-Glasrezept (`RecipeRole::AppIcons`) |

`appearance`: `0` = Light, `1` = Dark.

---

## Weg 1: Desktop-Shell / Overlay (Hyprland + Quickshell / Layer-Shell)

Unter Wayland sieht nur der Compositor den echten Desktop-Hintergrund hinter deinem Fenster. Das Hyprland-Plugin (`liquid-glass-hyprland.so`) übernimmt Brechung, Blur, Mitosis und Dual-Tint zentral auf der GPU.

### Bauen & Laden
```sh
make -C integrations/hyprland
mkdir -p ~/.local/share/liquidglass
cp integrations/hyprland/liquid-glass-hyprland.so ~/.local/share/liquidglass/liquid-glass-hyprland.so
hyprctl plugin load ~/.local/share/liquidglass/liquid-glass-hyprland.so
```
*(Wichtig beim Aktualisieren: nie über eine geladene `.so` kopieren – immer erst `hyprctl plugin unload`, dann `mv`, dann `hyprctl plugin load`.)*

### 1. Layer-Namespace setzen
Jede Layer-Shell-Fläche bekommt eine freie Integer-ID (`0..1023`, z. B. `870`):
```
liquidglass-rust-widget~870
```

### 2. Stil per Hyprland-IPC setzen
```text
/liquid-glass-style <ID> <role> <appearance> <radius> <diffusion> <r> <g> <b> <strength> [x y w h] [grad r2 g2 b2 s2 startPct endPct]
```
- `<radius>`: Eckradius in Punkten (Integer, z. B. `28` oder `width/2` für Kreis/Pille).
- `<diffusion>`: `0..100` (Hintergrundunschärfe / Legibility).
- `<r> <g> <b> <strength>`: Primär-Tint (`0..255`, Stärke `0..100`). `0 0 0 0` = kein Tint.
- `[x y w h]` *(optional für `role = 0, 1, 4`, Pflicht für `role = 3`)*: Lokale Bounds innerhalb eines festen Fensters (verhindert Wayland-Resize-Lag bei Animationen!).
- `[grad r2 g2 b2 s2 startPct endPct]` *(optional)*: **Vertikaler Dual-Tint** von `tint` (oben) zu `tint2` (unten) zwischen `startPct %` und `endPct %` der Höhe. Bei `s2 = 0` blendet der Shader auch die `Clear`-Verdunklung sauber aus → **oben tiefschwarz, unten 100 % kristallklares Brechungsglas** (Siri-Look):
  ```sh
  hyprctl liquid-glass-style 870 1 0 58 0 0 0 0 98 700 400 116 116 grad 0 0 0 0 24 58
  ```
- Ausschalten: `hyprctl liquid-glass-style 870 off`

### 3. Optional: GPU-Mitosis / Tropfen-Verschmelzung
Für bis zu 4 zusätzliche Ellipsen, die per `sdfUnion` flüssig mit der Hauptform verschmelzen (z. B. Spotlight-Buttons oder Siri-Randtropfen):
```text
/liquid-glass-icons <ID> <appearance> <diffusion> <r> <g> <b> <strength> <x1 y1 w1 h1> [x2 y2 w2 h2 ...]
/liquid-glass-icons <ID> off
```
*(Koordinaten sind immer Top-Left `x y w h` relativ zur Layer-Fläche.)*

### Minimalbeispiel in QML (mit `LiquidGlass.qml`)
In `liquidglass-quickshell` steht dafür der Wrapper `modules/LiquidGlass.qml` bereit:
```qml
import QtQuick
import Quickshell.Wayland

PanelWindow {
    id: win
    color: "transparent"
    implicitWidth: 320; implicitHeight: 120
    WlrLayershell.namespace: glass.layerNamespace

    LiquidGlass {
        id: glass
        glassId: 870
        active: win.visible
        role: "clear"             // "regular" | "clear" | "widget" | "dock" | "popover" | "appIcons"
        dark: false
        cornerRadius: 60
        diffusion: 0
        tint: "#000000"
        tintStrength: 0.98
        gradientEnabled: true
        bottomTint: "#000000"
        bottomTintStrength: 0.0
        gradientStart: 0.24
        gradientEnd: 0.58
    }
}
```

---

## Weg 2: Eigenständige App / Tool (z. B. Icon Composer, Rust / C++)

Wenn deine App Glas über ihren **eigenen** Canvas rendert (z. B. ein App-Icon-Editor mit geschichteten Glas-Ebenen und PNG-Export), brauchst du **kein Hyprland und kein QML**.

### C-ABI (`integrations/hyprland/widget_renderer.hpp`)
Linke gegen `integrations/hyprland/renderer/target/release/libliquid_glass_widget_renderer.a`:

1. **Echtzeit-GPU-Viewport (OpenGL / Vulkan / wgpu):**
   ```cpp
   #include "widget_renderer.hpp"

   Widget w{
       .width = 512, .height = 512, .scale = 2.0f,
       .bounds = {64, 64, 384, 384},
       .radius = 44.0f, .diffusion = 0.0f,
       .role = 7,           // 7 = AppIcons, 1 = Clear, 0 = Regular
       .appearance = 0,     // 0 = Light, 1 = Dark
       .opacity = 1.0f,
       .tint = {0.0f, 0.0f, 0.0f, 0.95f},
       .pointer = 0.5f,
       .tint2 = {0.0f, 0.0f, 0.0f, 0.0f},   // Optional: 2. Stop für Dual-Tint
       .tintStops = {0.24f, 0.58f},
   };
   PreparedWidget prepared{};
   if (lg_widget_prepare(&w, &prepared)) {
       // `prepared.uniforms`, `prepared.geometry`, `prepared.widgetTint`,
       // `prepared.widgetTint2`, `prepared.widgetTintAxis`, `prepared.highlightMatrix`
       // direkt an den generierten Material-Shader (widget_shaders.hpp) übergeben.
   }
   ```
2. **Direktes CPU-/Offscreen-Rendering für PNG-Export:**
   ```cpp
   WidgetRenderer* r = lg_widget_renderer_create();
   // `rgba` enthält den Hintergrund-Canvas (width * height * 4 Bytes RGBA8)
   // und wird in-place mit dem gerenderten Liquid-Glass-Ergebnis überschrieben.
   lg_widget_render(r, &w, rgba.data(), rgba.size());
   lg_widget_renderer_destroy(r);
   ```

### Direkt in Rust
In `Cargo.toml` einfach die lokale Crate `liquid_glass_core` (bzw. `integrations/hyprland/renderer`) einbinden – alle Materialrezepte (`Regular`, `Clear`, `Dock`, `Popover`, `AppIcons`), Highlight-Matrizen und `Frame`/`Renderer`-Typen sind direkt aus Rust nutzbar.
