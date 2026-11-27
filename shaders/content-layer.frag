#version 450
layout(location=0) in vec2 sourceUV;
layout(location=0) out vec4 color;
layout(binding=3) uniform sampler2D sourceImage;
layout(std140,binding=1) uniform Registration { vec4 row0;vec4 row1; } u;
void main() {
    vec3 p=vec3(sourceUV,1);
    vec2 uv=vec2(dot(u.row0.xyz,p),dot(u.row1.xyz,p));
    color=texture(sourceImage,uv);
}
