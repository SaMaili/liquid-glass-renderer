#version 450
// DesignLibrary 26A434 tint stops and interpolation; QuartzCore AIR 0232
// backdrop_aware_vibrant_color_matrix_sover. The group clip is a scene policy.
layout(location=0) out vec4 outputColor;
layout(binding=3) uniform sampler2D renderedGlass;
layout(binding=4) uniform sampler2D filteredSdf;
layout(binding=5) uniform sampler2D groupSdf;
layout(std140,binding=1) uniform Tint {
    mat4 matrix;
    vec4 bias;
    vec4 ramp;   // opaque distance, transparent distance, opacity, clamp maximum
    vec4 region; // raster origin x/y, sharp, preserve hue
} tint;

float coverage(float distance, float width) {
    return clamp(0.5-distance/width,0.0,1.0);
}

float cubic(float t, float p1, float p2) {
    float s=1.0-t;
    return 3.0*s*t*(s*p1+t*p2)+t*t*t;
}

float fade(float position) {
    if(position<=0.0) return 1.0;
    if(position>=1.0) return 0.0;
    // Evaluate the recovered cubic directly in place of Metal's sampled ramp.
    float lo=0.0, hi=1.0;
    for(int i=0;i<16;++i) {
        float mid=(lo+hi)*0.5;
        if(cubic(mid,0.60938,0.47124)<position) lo=mid;
        else hi=mid;
    }
    return 1.0-cubic((lo+hi)*0.5,0.00663,0.99115);
}

void main() {
    ivec2 p=ivec2(gl_FragCoord.xy);
    ivec2 sdfPixel=clamp(p,ivec2(0),textureSize(groupSdf,0)-1);
    vec4 group=texelFetch(groupSdf,sdfPixel,0);
    vec4 shape=texelFetch(filteredSdf,sdfPixel,0);
    float groupWidth=max(fwidthFine(group.r),0.0001);
    float shapeWidth=clamp(fwidthFine(shape.r),0.0001,1.0);
    float groupCoverage=coverage(group.r,groupWidth)*clamp(group.a,0.0,1.0);
    float shapeCoverage=coverage(shape.r-tint.ramp.y,shapeWidth);
    if(tint.region.z==0.0) {
        float position=(shape.r-tint.ramp.x)/(tint.ramp.y-tint.ramp.x);
        shapeCoverage*=fade(position);
    }
    shapeCoverage*=clamp(shape.a,0.0,1.0);
    float mask=min(groupCoverage,shapeCoverage)*tint.ramp.z;

    vec4 dst=texelFetch(renderedGlass,p-ivec2(tint.region.xy),0);
    if(mask==0.0) {
        outputColor=dst;
        return;
    }
    vec4 mapped=tint.matrix*vec4(dst.rgb/max(dst.a,0.000001),dst.a)+tint.bias;
    float alpha=clamp(mapped.a,0.0,1.0)*mask;
    vec3 rgb=mapped.rgb*alpha;
    float maximum=tint.ramp.w;
    if(maximum>0.0) {
        rgb/=max(alpha,0.000001);
        if(tint.region.w>0.0) {
            float peak=max(rgb.r,max(rgb.g,rgb.b));
            if(peak>maximum) rgb*=maximum/peak;
        } else {
            rgb=clamp(rgb,-0.75,maximum);
        }
        rgb*=alpha;
    }
    outputColor=vec4(rgb,alpha)+dst*(1.0-alpha);
}
