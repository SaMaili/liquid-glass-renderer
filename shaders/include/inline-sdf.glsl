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
