#version 450
// Semantic port of glass_foreground_lpf, module-0073, build 26A434.
// Host sampler state, fwidth quad behavior and Metal fast-math are unvalidated.
layout(location=0) in vec2 sourceUV;
layout(location=1) in vec2 sdfUV;
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D sourceTexture;
layout(binding=4) uniform sampler2D sdfTexture;
layout(std140,binding=1) uniform Foreground {
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
void main() {
    vec4 sdf=texture(sdfTexture,sdfUV);
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
