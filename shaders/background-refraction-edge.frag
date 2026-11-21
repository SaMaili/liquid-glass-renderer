#version 450
// Semantic port of module-0060, glass_background_re_lpf, build 26A434.
// Original 320-byte uniforms are represented as raw words, including half4s.
// Combined refraction and edge variant, including the specialized aberration helper.
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
float displacement(float distance,float amount,float invHeight) {
    float t=sat(-distance*invHeight);
    return amount-amount*sat(sqrt((2.0-t)*t));
}
float lodForDistance(float distance) {
    vec3 a=vec3(U(176),U(180),U(184));
    vec3 b=vec3(U(180),U(184),U(188));
    // Use the SDR reference convention for a collapsed blur interval.
    vec3 range=b-a;
    bvec3 collapsed=equal(range,vec3(0));
    vec3 span=mix(range,vec3(1),collapsed);
    vec3 t=mix(clamp(fma(vec3(distance),1.0/span,-a/span),0.0,1.0),vec3(0),collapsed);
    float radius=(U(160)-dot(vec3(U(164),U(168),U(172)),t))*U(40);
    return max(0.0,log2(radius<2.0 ? radius*0.5+1.0 : radius));
}
vec4 cleanSample(vec2 uv,float lod) {
    vec4 c=textureLod(sourceTexture,uv,lod);
    for(int i=0;i<3;++i) if(abs(c[i])<0.000001) c[i]=0.0;
    return c;
}
float lodForRadius(float radius) {
    return max(0.0,log2(radius<2.0 ? radius*0.5+1.0 : radius));
}
vec3 unpremultipliedSample(vec2 uv,float radius) {
    vec4 c=textureLod(sourceTexture,uv,lodForRadius(radius));
    vec3 rgb=c.rgb/max(c.a,0.000001);
    for(int i=0;i<3;++i) if(abs(rgb[i])<0.000001) rgb[i]=0.0;
    return rgb;
}
vec4 fourTap(vec2 uv,float lod) {
    vec2 o=vec2(lod*0.25)/vec2(textureSize(sourceTexture,0));
    vec4 c=(textureLod(sourceTexture,uv+o,lod)+textureLod(sourceTexture,uv+vec2(o.x,-o.y),lod)
        +textureLod(sourceTexture,uv+vec2(-o.x,o.y),lod)+textureLod(sourceTexture,uv-o,lod))*0.25;
    for(int i=0;i<3;++i) if(abs(c[i])<0.000001) c[i]=0.0;
    return c;
}
vec4 aberrate(vec2 uv,vec2 offset,float lod) {
    vec4 sum=vec4(0);float t=1.0;
    for(int i=0;i<3;++i) {
        vec4 c=fourTap(uv+t*offset,lod);
        sum.rg+=c.rg/max(c.a,0.000001)*vec2(t,1.0-t);sum.a+=c.a;t-=0.3333333432674408;
    }
    t=0.0;
    for(int i=0;i<4;++i) {
        vec4 c=fourTap(uv-t*offset,lod);
        sum.gb+=c.gb/max(c.a,0.000001)*vec2(1.0-t,t);sum.a+=c.a;t+=0.3333333432674408;
    }
    float alpha=sum.a*0.1428571492433548;
    vec3 rgb=sum.rgb*vec3(0.5,0.3333333432674408,0.5);
    // Literal module-0060 helper SSA %87..%98: RG multiplied by alpha,
    // B left straight, returned alpha squared. Do not normalize this away.
    return vec4(rgb.rg*alpha,rgb.b,alpha*alpha);
}
void main() {
    vec4 sdf=texture(sdfTexture,sdfUV);
    // Use derivatives from the current row and column of the fragment quad.
    float width=max(fwidthFine(sdf.r),0.0001);
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
    // The minimal IR performs a conditional ring SDF read whose value is
    // unused by its specialized base function. No ring contribution here.
    vec4 face=vec4(0);
    if(coverage>0.0) {
        float inner=displacement(sdf.r,U(16),U(20));
        vec4 sampleColor=cleanSample(sourceUV+inner*gradient,lodForDistance(sdf.r+inner));
        if(U(308)!=0.0) {
            vec2 dir=U2(312),a=U2(0),b=U2(8);
            vec2 row0=vec2(dot(a,dir),dir.x*a.y-dir.y*a.x);
            vec2 row1=vec2(dot(b,dir),dir.x*b.y-dir.y*b.x);
            vec2 offset=vec2(dot(row1,sdf.gb),dot(row0,sdf.gb));
            float t=sat((-sdf.r-E(4))*E(0));
            offset*=U(308)*(1.0-sat(sqrt((2.0-t)*t)));
            sampleColor=aberrate(sourceUV+inner*gradient,offset,lodForDistance(sdf.r+inner));
        }
        face=vec4(sampleColor.rgb/max(sampleColor.a,0.000001),1.0);
        if(U(224)>0.0) {
            float outer=displacement(sdf.r,U(24),U(28));
            vec4 outerColor=cleanSample(sourceUV+outer*gradient,lodForDistance(sdf.r+outer));
            float weight=sat(fma(sdf.r,1.0/(U(36)-U(32)),-U(32)/(U(36)-U(32))))*U(224);
            // Original outer sample remains premultiplied here (SSA %200/%214).
            face=mix(face,outerColor,weight);
        }
        vec3 rgb=face.rgb;
        if(U(204)>0.0) {
            vec3 adjusted=rgb;
            if(E(16)>0.0) {
                float luma=dot(vec3(0.2125999927520752,0.7152000069618225,0.0722000002861023),rgb);
                float c=sat(1.0-luma*E(16));
                adjusted=mix(vec3(c*luma),c*rgb,vec3(1.0+(1.0-c)*0.30000001192092896));
            }
            vec4 r=H4(80),g=H4(88),b=H4(96);
            vec3 mapped=vec3(dot(adjusted,r.rgb)+r.a,dot(adjusted,g.rgb)+g.a,dot(adjusted,b.rgb)+b.a);
            face=vec4(mix(rgb,mapped,U(204)),face.a);
        }
        // module-0060 edge branch: luminance-modulated edge bleed.
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
