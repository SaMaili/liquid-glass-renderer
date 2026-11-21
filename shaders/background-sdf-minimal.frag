#version 450
// Inline-SDF wrapper from modules 0065–0072; material below is a semantic port of module-0057, glass_background_minimal_lpf, build 26A434.
// Original 320-byte uniforms are represented as raw words, including half4s.
// This variant does NOT implement the richer background refraction/highlights.
layout(location=1) in vec2 sourceUV;
layout(location=0) in vec2 sdfUV;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;

layout(std140,binding=1) uniform Background { uvec4 words[23]; } u;
layout(std140,binding=9) uniform Extension { uvec4 words[2]; } ext;
layout(std140,binding=6) uniform EDR { float edr_scale; } e;
uint word(int byteOffset) { byteOffset+=48; return u.words[byteOffset/16][(byteOffset%16)/4]; }
float U(int byteOffset) { return uintBitsToFloat(word(byteOffset)); }
vec2 U2(int byteOffset) { return vec2(U(byteOffset),U(byteOffset+4)); }
vec4 H4(int byteOffset) { return vec4(unpackHalf2x16(word(byteOffset)),unpackHalf2x16(word(byteOffset+4))); }
float E(int byteOffset) { return uintBitsToFloat(ext.words[byteOffset/16][(byteOffset%16)/4]); }
vec4 geometry(int i);
vec4 computeSdf(vec2 p,int mode);
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
vec4 geometry(int i) { return uintBitsToFloat(u.words[i]); }
// AIR module 0065 compute_sdf_with_mode and supercircle_sdf, build 26A434.
// geometry(0..2) is supplied by the host shader. Float path, including the
// original half rounding in supercircle. Zero-radius/zero-normal inputs
// retain the original unresolved fast-math degeneracies.
float sdfHalf(float x) { return unpackHalf2x16(packHalf2x16(vec2(x,0))).x; }
vec3 supercircleSdf(vec2 p, vec2 extent, float radius, vec2 blend) {
    float r=abs(radius), scaled=r*1.5286649465560913;
    float reach=mix(scaled,r,max(blend.x,blend.y));
    vec2 q=p-extent+reach;
    vec2 z=abs(max(vec2(0),(p-extent+scaled)/scaled));
    float len=length(z), hi=max(z.x,z.y), lo=min(z.x,z.y);
    float t=hi==0.0 ? 0.0 : clamp(lo/hi,0.0,1.0);
    float poly=((((3.1560099124908447-t*0.9260540008544922)*t
                 -3.6412200927734375)*t+1.268030047416687)*t+0.2685309946537018);
    float curve=len+1.0-1.0/(1.0-t*t*clamp(len,0.0,1.0)*poly);
    vec2 rounded=max(vec2(0),z*1.5286649465560913-0.5286650061607361);
    float circle=length(rounded)*0.6541655659675598+0.3458344340324402;
    float direction=z.y>z.x ? 1.0 : -1.0;
    float w=clamp(0.5-direction+direction*t,0.0,1.0);
    float d=scaled*sdfHalf(mix(mix(curve,circle,blend.x),mix(curve,circle,blend.y),w)-1.0);
    vec2 positive=max(vec2(0),q);
    vec2 gradient=positive*inversesqrt(dot(positive,positive));
    vec2 axis=q.x>q.y ? vec2(1,0) : vec2(0,1);
    gradient=gradient.x+gradient.y>0.0 ? gradient : axis;
    return vec3(min(max(q.x,q.y),0.0)+d,gradient);
}
vec4 computeSdf(vec2 p, int mode) {
    vec4 a=geometry(0), m=geometry(1), b=geometry(2);
    vec2 signP=vec2(p.x>=0.0?1.0:-1.0,p.y>=0.0?1.0:-1.0);
    vec3 result;
    if(mode<4) {
        vec2 q=abs(p)-a.xy;
        result=vec3(max(q.x,q.y),(q.x>q.y?vec2(1,0):vec2(0,1))*signP);
    } else if(mode==4) {
        result=supercircleSdf(abs(p),a.xy,b.z,b.xy);
        result.yz*=signP;
    } else {
        vec4 avg=(b.xxwy+b.ywzz)*0.5;
        vec4 weight=clamp((-1.5286649465560913-a.xyxy/avg)*-1.891557216644287,0.0,1.0);
        weight=mix(vec4(1),weight,lessThan(avg,vec4(0)));
        result=supercircleSdf(p,a.xy,b.y,weight.xw);
        vec3 next=supercircleSdf(vec2(-p.x,p.y),a.xy,b.x,weight.xy);
        next.y=-next.y;
        if(next.x>result.x) result=next;
        next=supercircleSdf(vec2(p.x,-p.y),a.xy,b.z,weight.zw);
        next.z=-next.z;
        if(next.x>result.x) result=next;
        next=supercircleSdf(-p,a.xy,b.w,weight.zy);
        next.yz=-next.yz;
        if(next.x>result.x) result=next;
    }
    // A zero blend weight selects the shape normal even at the radial origin.
    vec2 shapeNormal=result.yz;
    if(a.w!=0.0) {
        vec2 radial=normalize(vec2(p.x,a.x*p.y/a.y));
        shapeNormal=mix(shapeNormal,radial,a.w);
    }
    vec2 gradient=normalize(shapeNormal);
    return vec4(result.x,dot(gradient,m.xy),dot(gradient,m.zw),1);
}

void main() {
    vec4 sdf=(int(geometry(0).z)>=0 ? computeSdf(sdfUV,int(geometry(0).z)) : vec4(0));
    float width=max(fwidth(sdf.r),0.0001);
    float coverage=sat(0.5-sdf.r/width)*sdf.a;
    vec4 shadow=vec4(0);
    if(coverage<1.0) {
        vec4 shifted=computeSdf(sdfUV+U2(64),abs(int(geometry(0).z))|4);
        float strength=U(220)*shifted.a*shadowProfile(U(76)*shifted.r);
        if(coverage==0.0 && strength<0.000001) discard;
        shadow=vec4(H4(128).a,H4(136).a,H4(144).a,U(156))*strength;
    }
    // The minimal IR performs a conditional ring SDF read whose value is
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
