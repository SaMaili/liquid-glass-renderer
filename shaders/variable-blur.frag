#version 450
// Arithmetic from AIR 0037. Texture/sampler bindings are host adapters.
layout(location=0) in vec2 uv;
layout(location=1) in vec2 maskUV;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(binding=4) uniform sampler2D maskTexture;
layout(binding=5) uniform sampler2D noiseTexture;
layout(std140,binding=9) uniform Noise {vec4 value;} noise;
layout(std140,binding=1) uniform Params { vec4 p0;vec4 p1;vec4 p2; } u;
layout(std140,binding=6) uniform EDR {vec4 value;} edr;
float level(float r){return max(0.0,log2(r<2.0?1.0+r*.5:r));}
vec4 sample4(vec2 p,float l){
 vec2 d=l*u.p1.xy;
 vec4 a=textureLod(sourceTexture,p+vec2(d.x,-d.y),l)+textureLod(sourceTexture,p+d,l);
 a+=textureLod(sourceTexture,p+vec2(-d.x,d.y),l);
 a+=textureLod(sourceTexture,p-d,l);return a*.25;
}
vec3 clean(vec3 c){return mix(c,vec3(0),lessThan(abs(c),vec3(1e-6)));}
void main(){
 bool compare=any(greaterThan(u.p2.yzw,vec3(0)));
 vec4 mask=texture(maskTexture,maskUV);
 vec2 m=clamp(compare?mask.rg:mask.aa,0.0,1.0);
 float radius=u.p0.x*m.x,l=level(radius);
 vec4 c=sample4(uv,l);c.rgb=clean(c.rgb);
 if(u.p0.z==2.0)c/=max(.001,sample4(uv+vec2(0,u.p0.w),l).a);
 else if(u.p0.z==1.0)c=vec4(c.rgb/max(.001,c.a),1);
 if(compare){
  float l2=level(u.p2.x);vec2 d=l2*u.p1.xy;
  vec4 other=(textureLod(sourceTexture,uv-d,l2)+textureLod(sourceTexture,uv+d,l2))*.5;
  vec3 o=clean(other.rgb/max(other.a,1e-6)),base=c.rgb/max(c.a,1e-6);
  float mn=m.y*u.p2.y,mx=m.y*u.p2.z;
  vec3 v=mx*max(base,o)+mn*min(base,o)+(1.0-mn-mx)*base;
  c.rgb=mix(v,o,u.p2.w*m.y)*c.a;
 }
 c*=clamp(u.p1.z*radius+u.p1.w,0.0,1.0);
 c.rgb*=edr.value.x;
 if(u.p0.y!=0.0)c.rgb+=noise.value.x*c.a*(texture(noiseTexture,gl_FragCoord.xy/32.0).rgb-.5);
 outputColor=c;
}
