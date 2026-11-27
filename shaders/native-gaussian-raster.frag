#version 450
// AIR0020/21/26/27/28..34: original float kernels and captured uniforms.
layout(location=0) in vec2 uv;
layout(location=0) out vec4 color;
layout(binding=3) uniform sampler2D sourceImage;
layout(std140,binding=1) uniform Pass {
 vec4 offsets[4];vec4 weights[2];vec4 divide;vec4 geometry[8];ivec4 mode;
} u;
vec2 off(int i){return i%2==0?u.offsets[i/2].xy:u.offsets[i/2].zw;}
float weight(int i){return u.weights[i/4][i%4];}
void main(){
 int op=u.mode.x;vec4 sum=vec4(0);
 if(op==0){color=sum;return;}
 if(op==1){color=texture(sourceImage,uv);return;}
 if(op==69){vec4 field=texture(sourceImage,uv);if(field.a<.5)discard;
 color=vec4(field.r/field.a,normalize(field.gb),1);return;}
 if(op==27){for(int i=0;i<4;i++)sum=fma(textureLod(sourceImage,uv+off(i),0),vec4(weight(i)),sum);}
 else if(op==29){for(int s=0;s<4;s++)for(int i=0;i<4;i++){vec2 d=off(i);if(s==1)d.y=-d.y;if(s==2)d.x=-d.x;if(s==3)d=-d;sum=fma(textureLod(sourceImage,uv+d,0),vec4(.0625),sum);}}
 else if(op==31||op==32){int n=op==31?3:6;sum=texture(sourceImage,uv)*weight(0);for(int i=0;i<n;i++)sum+=(texture(sourceImage,uv+off(i))+texture(sourceImage,uv-off(i)))*weight(i+1);}
 else if(op==45){for(int i=0;i<4;i++)sum+=texture(sourceImage,uv+off(i));sum*=weight(0);}
 else {int pairs=op-31;for(int i=0;i<pairs;i++)sum+=(texture(sourceImage,uv+off(i))+texture(sourceImage,uv-off(i)))*weight(i);}
 if(op>=33&&u.divide.x!=0)sum/=max(.001,sum.a);
 color=sum;
}
