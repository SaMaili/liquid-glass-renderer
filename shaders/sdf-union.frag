#version 450
// AIR module 0239, ShaderUtils_<float>::sdf_union, build 26A434.
// Texture fetch wrapper is ours; arithmetic below follows the recovered helper.
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D inputA;
layout(binding=4) uniform sampler2D inputB;
layout(std140,binding=1) uniform Union { vec4 parameters; } u;
vec4 sdfUnion(vec4 a,vec4 b,float smoothness) {
    if(b.w==0.0) b.xyz=vec3(10000.0,0.0,0.0);
    float k=clamp(0.5-0.5*dot(a.yz,b.yz),0.0,1.0)*smoothness;
    float h=clamp(0.5+0.5*(b.x-a.x)/k,0.0,1.0);
    return vec4(mix(b.x,a.x,h)-k*h*(1.0-h),mix(b.yz,a.yz,h),1.0);
}
void main() {
    ivec2 p=ivec2(gl_FragCoord.xy);
    outputColor=sdfUnion(texelFetch(inputA,p,0),texelFetch(inputB,p,0),u.parameters.x);
}
