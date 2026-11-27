#version 450
// AIR0028..0034: original NarrowBlurUniforms byte layout.
layout(location=0) in vec2 sourceUV;
layout(location=0) out vec4 color;
layout(binding=3) uniform sampler2D sourceImage;
layout(std140,binding=1) uniform NarrowBlur {
    vec4 offsets[4];
    vec4 weights[2];
    vec4 division;
} u;
layout(std140,binding=6) uniform EDR { vec4 scale; } e;
layout(std140,binding=10) uniform Variant { ivec4 variant; } v;
vec2 offsetAt(int i) { return (i%2==0) ? u.offsets[i/2].xy : u.offsets[i/2].zw; }
float weightAt(int i) { return u.weights[i/4][i%4]; }
void main() {
    vec4 sum=vec4(0);
    if (v.variant.x==3) {
        sum=texture(sourceImage,sourceUV+offsetAt(0));
        sum+=texture(sourceImage,sourceUV+offsetAt(1));
        sum+=texture(sourceImage,sourceUV+offsetAt(2));
        sum+=texture(sourceImage,sourceUV+offsetAt(3));
        sum*=weightAt(0);
    } else {
        int pairs=(v.variant.x+1)/4;
        for(int i=0;i<pairs;i++) {
            vec2 d=offsetAt(i);
            sum+=(texture(sourceImage,sourceUV+d)+texture(sourceImage,sourceUV-d))*weightAt(i);
        }
    }
    if(u.division.x!=0) sum/=max(0.001,sum.a);
    color=vec4(sum.rgb*e.scale.x,sum.a);
}
