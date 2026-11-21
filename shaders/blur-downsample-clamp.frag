#version 450
// AIR module 0036, build 26A434. Explicit 13-sample variable-blur pass.
layout(location=0) in vec2 uv;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(std140,binding=1) uniform Blur { vec4 parameters; vec4 bounds; } u;
vec4 sampleAt(vec2 p) { return texture(sourceTexture,clamp(p,u.bounds.xy,u.bounds.zw)); }
void main() {
    vec2 d=u.parameters.yz;
    float inner=1.9600850343704224, outer=3.9206759929656982;
    vec4 axial=sampleAt(uv+vec2(0,-inner*d.y))+sampleAt(uv+vec2(-inner*d.x,0));
    axial+=sampleAt(uv+vec2(0,inner*d.y)); axial+=sampleAt(uv+vec2(inner*d.x,0));
    vec4 diagonal=sampleAt(uv+vec2(-inner*d.x,inner*d.y))+sampleAt(uv-inner*d);
    diagonal+=sampleAt(uv+vec2(inner*d.x,-inner*d.y));diagonal+=sampleAt(uv+inner*d);
    vec4 farSamples=sampleAt(uv+vec2(0,-outer*d.y))+sampleAt(uv+vec2(-outer*d.x,0));
    farSamples+=sampleAt(uv+vec2(0,outer*d.y));farSamples+=sampleAt(uv+vec2(outer*d.x,0));
    outputColor=axial*0.09018599987030029+sampleAt(uv)*0.10549700260162354;
    outputColor+=diagonal*0.07709799706935883;
    outputColor+=farSamples*0.05634099990129471;
}
