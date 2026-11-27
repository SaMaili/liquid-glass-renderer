#version 450
// AIR0035/0036 plus the proven white single-source image-copy specialization.
layout(location=0) in vec2 uv;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(std140,binding=1) uniform Quad {
 vec4 parameters;vec4 bounds;vec4 texmat;mat4 mvp;
 vec4 positions[4];vec4 coordinates[4];uvec4 mode;
} u;
vec4 sampleAt(vec2 p){
 if(u.mode.x==40u||u.mode.x==1u&&u.mode.y==5u)p=clamp(p,u.bounds.xy,u.bounds.zw);
 return textureLod(sourceTexture,p,u.parameters.x);
}
void main(){
 if(u.mode.x==1u){outputColor=sampleAt(uv);return;}
 vec2 d=u.parameters.yz;float inner=1.9600850343704224,outer=3.9206759929656982;
 vec4 a=sampleAt(uv+vec2(0,-inner*d.y))+sampleAt(uv+vec2(-inner*d.x,0));
 a+=sampleAt(uv+vec2(0,inner*d.y));a+=sampleAt(uv+vec2(inner*d.x,0));
 vec4 g=sampleAt(uv+vec2(-inner*d.x,inner*d.y))+sampleAt(uv-inner*d);
 g+=sampleAt(uv+vec2(inner*d.x,-inner*d.y));g+=sampleAt(uv+inner*d);
 vec4 f=sampleAt(uv+vec2(0,-outer*d.y))+sampleAt(uv+vec2(-outer*d.x,0));
 f+=sampleAt(uv+vec2(0,outer*d.y));f+=sampleAt(uv+vec2(outer*d.x,0));
 outputColor=a*.09018599987030029+sampleAt(uv)*.10549700260162354;
 outputColor+=g*.07709799706935883;outputColor+=f*.05634099990129471;
}
