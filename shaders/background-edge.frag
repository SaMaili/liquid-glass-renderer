#version 450
// Semantic port of module-0058, glass_background_e_lpf, build 26A434.
// Original 320-byte uniforms are represented as raw words, including half4s.
// Edge-bleed and source-dependent shadow variant; CPU selection is unresolved.
layout(location=0) in vec2 sourceUV;
layout(location=1) in vec2 sdfUV;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(binding=4) uniform sampler2D sdfTexture;
layout(std140,binding=1) uniform Background { uvec4 words[20]; } u;
layout(std140,binding=9) uniform Extension { uvec4 words[2]; } ext;
layout(std140,binding=6) uniform EDR { float edr_scale; } e;
uint word(int byteOffset) { return u.words[byteOffset/16][(byteOffset%16)/4]; }
float U(int byteOffset) { return uintBitsToFloat(word(byteOffset)); }
vec2 U2(int byteOffset) { return vec2(U(byteOffset),U(byteOffset+4)); }
vec4 H4(int byteOffset) { return vec4(unpackHalf2x16(word(byteOffset)),unpackHalf2x16(word(byteOffset+4))); }
float E(int byteOffset) { return uintBitsToFloat(ext.words[byteOffset/16][(byteOffset%16)/4]); }
float sat(float x) { return clamp(x,0.0,1.0); }
// Quantize each half operation in the original polynomial. GPU FMA behavior
// still needs comparison against Metal; binary16 storage alone is not proof.
float h(float x) { return unpackHalf2x16(packHalf2x16(vec2(x,0))).x; }
float hc(uint bits) { return unpackHalf2x16(bits).x; }
float shadowProfile(float d) {
    float x=h(sat(h(h(h(d)*0.25)+0.5)));
    x=h(h(x*4.0)-2.0);
    float x2=h(x*x);
    float p=h(fma(hc(0x1A0Du),x2,hc(0xA869u)));
    p=h(fma(p,x2,hc(0x3162u)));
    p=h(fma(p,x2,hc(0xB87Cu)));
    return h(fma(p,x,0.5));
}
float lodForRadius(float radius) {
    return max(0.0,log2(radius<2.0 ? radius*0.5+1.0 : radius));
}
float displacement(float distance,float amount,float invHeight) {
    float t=sat(-distance*invHeight);
    return amount-amount*sat(sqrt((2.0-t)*t));
}
vec3 unpremultipliedSample(vec2 uv,float radius) {
    vec4 c=textureLod(sourceTexture,uv,lodForRadius(radius));
    vec3 rgb=c.rgb/max(c.a,0.000001);
    for(int i=0;i<3;++i) if(abs(rgb[i])<0.000001) rgb[i]=0.0;
    return rgb;
}
void main() {
    vec4 sdf=texture(sdfTexture,sdfUV);
    float width=max(fwidth(sdf.r),0.0001);
    float coverage=sat(0.5-sdf.r/width)*sdf.a;
    vec2 gradient=vec2(dot(sdf.gb,U2(0)),dot(sdf.gb,U2(8)));
    vec4 shadow=vec4(0);
    if(coverage<1.0) {
        vec4 shifted=texture(sdfTexture,sdfUV+U2(64));
        float strength=U(220)*shifted.a*shadowProfile(U(76)*shifted.r);
        if(coverage==0.0 && strength<0.000001) discard;
        vec4 r=H4(128),g=H4(136),b=H4(144);
        vec3 color=vec3(r.a,g.a,b.a);
        float alpha=U(156);
        if(U(152)>0.000001) {
            float offset=displacement(sdf.r+U(216),U(56),U(60));
            vec3 rgb=unpremultipliedSample(sourceUV+offset*gradient,U(72));
            color+=U(152)*vec3(dot(rgb,r.rgb),dot(rgb,g.rgb),dot(rgb,b.rgb));
            alpha=mix(alpha,1.0,U(152));
        }
        shadow=vec4(color,alpha)*strength;
    }
    // This specialized IR performs a conditional ring SDF read whose value is
    // unused by its specialized base function. No ring contribution here.
    vec4 face=vec4(0);
    if(coverage>0.0) {
        float radius=U(160)*U(40);
        float lod=max(0.0,log2(radius<2.0 ? radius*0.5+1.0 : radius));
        vec4 sampleColor=textureLod(sourceTexture,sourceUV,lod);
        vec3 rgb=sampleColor.rgb;
        for(int i=0;i<3;++i) if(abs(rgb[i])<0.000001) rgb[i]=0.0;
        rgb/=max(sampleColor.a,0.000001);
        face=vec4(rgb,1.0);
        if(U(204)>0.0) {
            vec3 adjusted=rgb;
            if(E(16)>0.0) {
                float luma=dot(vec3(0.2125999927520752,0.7152000069618225,0.0722000002861023),rgb);
                float c=sat(1.0-luma*E(16));
                adjusted=mix(vec3(c*luma),c*rgb,vec3(1.0+(1.0-c)*0.30000001192092896));
            }
            vec4 r=H4(80),g=H4(88),b=H4(96);
            vec3 mapped=vec3(dot(adjusted,r.rgb)+r.a,dot(adjusted,g.rgb)+g.a,dot(adjusted,b.rgb)+b.a);
            face=vec4(mix(rgb,mapped,U(204)),1.0);
        }
        // module-0058 base SSA %241..%334: luminance-modulated edge bleed.
        float offset=displacement(sdf.r,U(48),U(52));
        vec3 bleed=unpremultipliedSample(sourceUV+offset*gradient,U(44));
        vec4 br=H4(104),bg=H4(112),bb=H4(120);
        bleed=vec3(dot(bleed,br.rgb)+br.a,dot(bleed,bg.rgb)+bg.a,dot(bleed,bb.rgb)+bb.a);
        float ramp=sat(fma(sdf.r,1.0/(U(196)-U(192)),-U(192)/(U(196)-U(192))));
        float luma=sat(dot(face.rgb,vec3(0.21250000596046448,0.7153999805450439,0.07209999859333038)));
        float darken=U(208)*luma+U(212);
        float weight=darken*darken*ramp;
        weight=weight*weight*U(200);
        face.rgb=mix(face.rgb,bleed,weight);
    }
    vec4 color=mix(shadow,face,coverage);
    if(U(228)>0.0) {
        float weight=(1.0-sat((sdf.r-U(232))*U(236)))*U(228);
        float alpha=sat(color.a);
        vec3 rgb=E(8)*color.rgb*alpha/max(color.a,0.000001);
        color=mix(color,vec4(rgb,alpha),weight);
    }
    if(E(12)>0.0) {
        vec3 rgb=color.rgb/max(color.a,0.000001);
        bool preserveHue=(ext.words[1].y & 255u)!=0u; // byte 20
        if(preserveHue) {
            float peak=max(rgb.r,max(rgb.g,rgb.b));
            if(peak>E(12)) rgb*=E(12)/peak;
        } else {
            rgb=clamp(rgb,vec3(-0.75),vec3(E(12)));
        }
        color.rgb=rgb*color.a;
    }
    outputColor=vec4(color.rgb*e.edr_scale,color.a);
}
