#version 450
// AIR0109: the half cache was expanded into a texture with a two-pixel halo.
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
vec4 h(vec4 v){return vec4(unpackHalf2x16(packHalf2x16(v.xy)),unpackHalf2x16(packHalf2x16(v.zw)));}
vec4 tap(ivec2 delta){return texelFetch(sourceTexture,ivec2(gl_FragCoord.xy)+2+delta,0);}
void main(){
 vec4 diagonal=h(tap(ivec2(1,-1))+tap(ivec2(-1,-1)));
 diagonal=h(diagonal+tap(ivec2(-1,1)));diagonal=h(diagonal+tap(ivec2(1,1)));
 vec4 axial=h(tap(ivec2(-1,0))+tap(ivec2(0,-1)));
 axial=h(axial+tap(ivec2(1,0)));axial=h(axial+tap(ivec2(0,1)));
 vec4 farTaps=h(tap(ivec2(-2,0))+tap(ivec2(0,-2)));
 farTaps=h(farTaps+tap(ivec2(2,0)));farTaps=h(farTaps+tap(ivec2(0,2)));
 vec4 a=h(diagonal*.07708740234375),b=h(tap(ivec2(0))*.10546875);
 vec4 c=h(axial*.0902099609375),d=h(farTaps*.05633544921875);
 vec4 v=h(h(h(a+b)+c)+d);
 float threshold=unpackHalf2x16(0x068eu).x;
 v.rgb=mix(v.rgb,vec3(0),lessThan(abs(v.rgb),vec3(threshold)));outputColor=v;
}
