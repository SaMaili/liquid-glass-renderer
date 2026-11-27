#version 450
// AIR0110/0111 per-output arithmetic. Explicit native pixel steps and dimensions.
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(std140,binding=1) uniform Params {vec4 sampling;uvec4 mode;} u;
vec4 h(vec4 v){return vec4(unpackHalf2x16(packHalf2x16(v.xy)),unpackHalf2x16(packHalf2x16(v.zw)));}
vec4 tap(vec2 d){return h(textureLod(sourceTexture,(floor(gl_FragCoord.xy)+.5+d)*u.sampling.xy,0));}
void main(){
 uvec2 p=uvec2(gl_FragCoord.xy);uvec2 tileExtent=min(uvec2(16,32),u.mode.yz-(p/uvec2(16,32))*uvec2(16,32));
 bool scalar=u.mode.x==6u&&all(equal(tileExtent,uvec2(1)));
 vec4 diagonal=h(tap(vec2(1,-1))+tap(vec2(-1,-1)));
 diagonal=h(diagonal+tap(vec2(-1,1)));diagonal=h(diagonal+tap(vec2(1,1)));
 vec4 axial=h(tap(vec2(-1,0))+tap(vec2(0,-1)));
 axial=h(axial+tap(vec2(1,0)));axial=h(axial+tap(vec2(0,1)));
 vec4 farTaps=h(tap(vec2(-2,0))+tap(vec2(0,-2)));
 farTaps=h(farTaps+tap(vec2(2,0)));farTaps=h(farTaps+tap(vec2(0,2)));
 if(scalar){
  diagonal=h(tap(vec2(-1,1))+tap(vec2(-1,-1)));
  diagonal=h(diagonal+tap(vec2(1,-1)));diagonal=h(diagonal+tap(vec2(1,1)));
  axial=h(tap(vec2(0,-1))+tap(vec2(-1,0)));
  axial=h(axial+tap(vec2(0,1)));axial=h(axial+tap(vec2(1,0)));
  farTaps=h(tap(vec2(0,-2))+tap(vec2(-2,0)));
  farTaps=h(farTaps+tap(vec2(0,2)));farTaps=h(farTaps+tap(vec2(2,0)));
 }
 vec4 a=h(diagonal*.07708740234375),b=h(tap(vec2(0))*.10546875);
 vec4 c=h(axial*.0902099609375),d=h(farTaps*.05633544921875);
 vec4 v=scalar?h(h(h(c+b)+a)+d):h(h(h(a+b)+c)+d);
 if(u.mode.x==6u){float threshold=unpackHalf2x16(0x068eu).x;v.rgb=mix(v.rgb,vec3(0),lessThan(abs(v.rgb),vec3(threshold)));}
 outputColor=v;
}
