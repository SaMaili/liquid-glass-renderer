"""Synthetic, reproducible renderer example. No claim of Apple calibration."""
import argparse,json,time
from pathlib import Path
import numpy as np
from PIL import Image
from .scene import SceneRenderer,Material,Element,BlurConfig


def demo(output,size=(384,216),frames=1):
    w,h=size;y,x=np.mgrid[:h,:w]
    image=np.ones((h,w,4),np.float32)
    checker=((x//12+y//12)%2).astype(float)
    image[...,0]=.04+.65*(x+.5)/w
    image[...,1]=.06+.45*checker
    image[...,2]=.1+.6*(y+.5)/h
    values=dict(displacement_mat=(1/w,0,0,1/h),refraction_amount=7.,
                inv_refraction_height=.08,refraction_offset=0.,aberration_amount=.4,
                inv_aberration_height=.1,aberration_offset=0.,aberration_dir=(1.,0.),
                edge_start=-1.,edge_end=1.,edge_opacity_start=0.,edge_opacity_end=0.,
                refraction_dir=(1.,0.))
    material=Material.from_values('foreground',values)
    geometry=[[w*.18,h*.23,4,0],[1,0,0,1],[1,1,h*.15,0]]
    elements=[Element('left',material,geometry=geometry,transform=[[w,0,-w*.37-.23],[0,h,-h*.48-.37]],group='glass'),
              Element('right',material,geometry=geometry,transform=[[w,0,-w*.64-.61],[0,h,-h*.53-.13]],group='glass')]
    durations=[]
    with SceneRenderer(size) as renderer:
        for _ in range(frames):
            start=time.perf_counter()
            result=renderer.render(image,elements,smoothness=h*.16,backdrop_mode='original',output_transfer='srgb')
            durations.append(time.perf_counter()-start)
        trace=renderer.last_trace
        programs=len(renderer.material_renderer.backend.programs)
        contexts=renderer.pool.created
    output=Path(output);output.parent.mkdir(parents=True,exist_ok=True)
    Image.fromarray(np.round(np.clip(result,0,1)*255).astype(np.uint8)).save(output)
    report={'scope':'Synthetic materials, reconstructed passes, explicit host grouping; not a native reference',
            'size':size,'frames':frames,'seconds':durations,'programs':programs,'additional_contexts':contexts,'passes':trace}
    output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--width',type=int,default=384);p.add_argument('--height',type=int,default=216)
    p.add_argument('--frames',type=int,default=1)
    args=p.parse_args()
    if not 1<=args.frames<=1000:p.error('frames must be 1..1000')
    print(json.dumps(demo(args.output,(args.width,args.height),args.frames)))


if __name__=='__main__':main()
