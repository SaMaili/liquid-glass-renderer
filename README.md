# Liquid Glass Renderer

Eigenständiger Offscreen-Renderer für Linux mit Rust-Bibliothek, Python-Paket und gemeinsamen GLSL-Shadern. Eine Anwendung liefert Hintergrundpixel, Ebenen und Materialparameter. Der Renderer zeichnet die Glasflächen, bricht und filtert den Hintergrund und gibt das zusammengesetzte Bild zurück.

## Installation und Start

Vorausgesetzt werden Linux, EGL und ein OpenGL-4.5-Treiber. Mesa mit llvmpipe funktioniert als Software-Renderer. Rust benötigt zusätzlich einen Linker sowie die Entwicklungspakete für EGL und GL, zum Beispiel `libegl-dev` und `libgl-dev` unter Debian oder Ubuntu. Der Rust-Stand wurde mit Rust 1.99 geprüft. Das Python-Paket benötigt Python 3.10+, NumPy und Pillow.

### Rust

```sh
cargo build --release --manifest-path rust/glass-core/Cargo.toml
mkdir -p out
cargo run --release --manifest-path rust/glass-core/Cargo.toml --example pass -- out/rust-glass.ppm
```

Das Beispiel rendert zwei überlappende Flächen und aktualisiert einen Ausschnitt des Hintergrunds. Für eigene Anwendungen bietet das Crate `liquid_glass_core` die Typen `Renderer`, `Frame`, `SdrRecipe` und `LiveScene`.

### Python

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install .
python -m liquid_glass --output out/python-glass.png
```

Mit `SceneRenderer`, `Element` und `Material` lassen sich eigene Szenen definieren. Das Python-Paket erwartet explizite Materialwerte. Arrays haben die Form `(Höhe, Breite, 4)`. Die Szenenkomposition arbeitet standardmäßig mit linearem, vormultipliziertem RGBA.

## Feature-Checkliste

- [x] Offscreen-Rendering mit EGL/OpenGL und gemeinsamen GLSL-Pässen
- [x] Distanzfelder aus Geometrie oder als Raster sowie weiche Vereinigung mehrerer Formen
- [x] Brechung, chromatische Aberration, Blur-Mip-Pyramide und Hintergrundkomposition
- [x] Konturglanz, Randfärbung und Schatten in den entsprechenden Materialpässen
- [x] Geordnete Glasgruppen über dem ursprünglichen oder bereits zusammengesetzten Hintergrund
- [x] Federbasierte Animation mit neuen Zielen während der Bewegung
- [x] Rust-SDR-Materialrezepte für Clear, Regular, Dock und Popover mit Diffusion und Tint
- [x] Rust-Interaktionsschicht mit Hover, Press und lokalem Lichtfleck
- [x] Rust-Teilaktualisierung von Hintergrundregionen und Wiederverwendung von GPU-Ressourcen
- [ ] Allgemeine automatische Material- und Formzuordnung wie im macOS-Compositor
- [ ] Vollständige visuelle Übereinstimmung mit nativen Apple-Ausgaben auf allen Grafiktreibern

Das Repository enthält den ausführbaren Renderer und synthetische Beispiele. Es ist ein unabhängiges Projekt mit Linux-Backend.
