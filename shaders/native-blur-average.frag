#version 450
// AIR0109 integer reads and half 2x2 cache values. The host retains the halo.
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(std140,binding=1) uniform Copy {ivec4 originMode;ivec4 bounds;} u;
vec4 h(vec4 v){return vec4(unpackHalf2x16(packHalf2x16(v.xy)),unpackHalf2x16(packHalf2x16(v.zw)));}
ivec2 s16(ivec2 p){return (p<<16)>>16;}
vec4 readSource(ivec2 p){
 p=s16(p);ivec2 lo=u.bounds.xy,hi=u.bounds.zw;
 bool valid=all(greaterThanEqual(p,lo))&&all(lessThanEqual(p,hi));
 if(u.originMode.w==2){
  p=ivec2(p.x<lo.x?s16(2*lo-p).x:p.x,p.y<lo.y?s16(2*lo-p).y:p.y);
  p=ivec2(p.x>hi.x?s16(2*hi-p).x:p.x,p.y>hi.y?s16(2*hi-p).y:p.y);
 }
 vec4 v=h(texelFetch(sourceTexture,clamp(p,lo,hi),0));
 return u.originMode.w==0&&!valid?vec4(0):v;
}
void main(){
 ivec2 p=ivec2(gl_FragCoord.xy);
 if(u.originMode.z==0){outputColor=readSource(p+u.originMode.xy);return;}
 p=2*(p-2)+u.originMode.xy;
 vec4 v=h(readSource(p+ivec2(0,1))+readSource(p+ivec2(1,1)));
 v=h(v+readSource(p+ivec2(1,0)));v=h(v+readSource(p));outputColor=h(v*.25);
}
