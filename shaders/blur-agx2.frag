#version 450
// AIR0111 per-output arithmetic; threadgroup caching replaced by direct fetches.
layout(location=0) in vec2 uv;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(std140,binding=1) uniform Params {vec4 p;} u;
vec4 half4(vec4 x){return vec4(unpackHalf2x16(packHalf2x16(x.xy)),unpackHalf2x16(packHalf2x16(x.zw)));}
vec4 tap(vec2 d){return half4(textureLod(sourceTexture,uv+d*u.p.xy,0));}
void main(){
 vec4 diagonal=half4(tap(vec2(1,-1))+tap(vec2(-1,-1)));
 diagonal=half4(diagonal+tap(vec2(-1,1)));diagonal=half4(diagonal+tap(vec2(1,1)));
 vec4 axial=half4(tap(vec2(-1,0))+tap(vec2(0,-1)));
 axial=half4(axial+tap(vec2(1,0)));axial=half4(axial+tap(vec2(0,1)));
 vec4 farTaps=half4(tap(vec2(-2,0))+tap(vec2(0,-2)));
 farTaps=half4(farTaps+tap(vec2(2,0)));farTaps=half4(farTaps+tap(vec2(0,2)));
 vec4 a=half4(diagonal*.07708740234375),b=half4(tap(vec2(0))*.10546875);
 vec4 c=half4(axial*.0902099609375),d=half4(farTaps*.05633544921875);
 outputColor=half4(half4(half4(a+b)+c)+d);
}
