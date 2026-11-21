#version 450
// Inline-SDF wrapper from module 0074; semantic port of glass_foreground_lpf, module-0073, build 26A434.
// Host sampler state, fwidth quad behavior and Metal fast-math are unvalidated.
layout(location=1) in vec2 sourceUV;
layout(location=0) in vec2 sdfUV;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;

layout(std140,binding=1) uniform Foreground {
    vec4 geometry_data[3];
    vec4 displacement_mat;
    float refraction_amount;
    float inv_refraction_height;
    float refraction_offset;
    float aberration_amount;
    float inv_aberration_height;
    float aberration_offset;
    vec2 aberration_dir;
    float edge_start;
    float edge_end;
    float edge_opacity_start;
    float edge_opacity_end;
    vec2 refraction_dir;
} u;
layout(std140,binding=6) uniform EDR { float edr_scale; } e;
float sat(float x) { return clamp(x,0.0,1.0); }
vec2 transformedGradient(vec2 direction, vec2 gradient) {
    vec2 a=u.displacement_mat.xy, b=u.displacement_mat.zw;
    vec2 row0=vec2(dot(a,direction), direction.x*a.y-direction.y*a.x);
    vec2 row1=vec2(dot(b,direction), direction.x*b.y-direction.y*b.x);
    return vec2(dot(row0,gradient),dot(row1,gradient));
}
float profile(float distance, float invHeight, float offset, float amount) {
    float t=sat((-distance-offset)*invHeight);
    return amount-amount*sat(sqrt((2.0-t)*t));
}
vec4 geometry(int i) { return u.geometry_data[i]; }
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
    vec4 sdf=computeSdf(sdfUV,int(abs(geometry(0).z)));
    float width=fwidth(sdf.r);
    if(sdf.a<0.001) { outputColor=vec4(0); return; }
    float coverage=sat(0.5-sdf.r/max(width,0.0001));
    vec2 refraction=transformedGradient(u.refraction_dir,sdf.gb)
        *profile(sdf.r,u.inv_refraction_height,u.refraction_offset,u.refraction_amount);
    // SSA %80/%82 swaps the two components for aberration only.
    vec2 aberration=transformedGradient(u.aberration_dir,sdf.gb).yx
        *profile(sdf.r,u.inv_aberration_height,u.aberration_offset,u.aberration_amount);
    vec2 center=sourceUV+refraction;
    vec4 accum=vec4(0);
    float t=1.0;
    for(int i=0;i<3;++i) {
        vec4 s=texture(sourceTexture,center+t*aberration);
        vec2 rg=s.rg/max(s.a,0.000001);
        accum.r+=rg.r*t;
        accum.g+=rg.g*(1.0-t);
        accum.a+=s.a;
        t+=-0.3333333432674408;
    }
    t=0.0;
    for(int i=0;i<4;++i) {
        vec4 s=texture(sourceTexture,center-t*aberration);
        vec2 gb=s.gb/max(s.a,0.000001);
        accum.g+=gb.x*(1.0-t);
        accum.b+=gb.y*t;
        accum.a+=s.a;
        t+=0.3333333432674408;
    }
    float alpha=sdf.a*0.1428571492433548*coverage*accum.a;
    vec3 rgb=accum.rgb*vec3(0.5,0.3333333432674408,0.5)*(alpha*e.edr_scale);
    float edge=sat(fma(sdf.r,1.0/(u.edge_end-u.edge_start),
                      -u.edge_start/(u.edge_end-u.edge_start)));
    outputColor=vec4(rgb,alpha)*(1.0-mix(u.edge_opacity_start,u.edge_opacity_end,edge));
}
