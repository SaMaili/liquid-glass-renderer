CXX ?= g++
CXXFLAGS += -O2 -fPIC -std=c++23 -fno-gnu-unique
HYPRLAND_FLAGS := $(shell pkg-config --cflags --libs hyprland pixman-1 libdrm egl glesv2)
RUST_LIB := renderer/target/release/libliquid_glass_widget_renderer.a
GPU := gpu_widget.cpp gpu_widget.hpp widget_renderer.hpp framebuffer_region.hpp
GENERATED := renderer/target/widget_shaders.hpp

.PHONY: all renderer check
all: liquid-glass-hyprland.so

renderer:
	cargo build --manifest-path renderer/Cargo.toml --locked --offline --release --target-dir renderer/target

$(GENERATED): embed_shaders.py ../../shaders/sdf-probe.frag ../../shaders/sdf-union.frag ../../shaders/blur-agx2.frag ../../shaders/background-all.frag
	mkdir -p $(@D)
	python embed_shaders.py $@

liquid-glass-hyprland.so: widget_pass.cpp $(GPU) $(GENERATED) renderer
	$(CXX) $(CXXFLAGS) -Irenderer/target -shared widget_pass.cpp gpu_widget.cpp $(RUST_LIB) $(HYPRLAND_FLAGS) -lGL -o $@

check: $(GPU) $(GENERATED) renderer
	$(CXX) $(CXXFLAGS) -Irenderer/target renderer_test.cpp gpu_widget.cpp $(RUST_LIB) -lEGL -lGLESv2 -lGL -o renderer/target/widget-renderer-test
	LIBGL_ALWAYS_SOFTWARE=1 renderer/target/widget-renderer-test
