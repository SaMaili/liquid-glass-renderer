"""Offscreen OpenGL execution of the GLSL ports using surfaceless EGL.

Synthetic fixtures only. Samplers use linear/clamp-to-edge and no mip chain;
this does not establish Apple's sampler state or original render graph.
Requires system EGL/OpenGL, Python numpy and Pillow. No window is opened.
"""
import ctypes as C
import json
import struct
import sys
from collections import OrderedDict
from pathlib import Path
import numpy as np
from PIL import Image

HERE = Path(__file__).resolve().parent

def dimensions(size):
    pair=(size,size) if type(size) is int else size
    if not isinstance(pair,(tuple,list)) or len(pair)!=2 or any(type(v) is not int or not 1<=v<=4096 for v in pair):
        raise ValueError('size must be 1..4096 or (width,height) in that range')
    return tuple(pair)


def bind(lib, name, result, *args):
    f = getattr(lib, name)
    f.restype, f.argtypes = result, args
    return f


class Renderer:
    def __init__(self, size=128):
        self.size = size
        self.width,self.height=dimensions(size)
        self.egl = C.CDLL('libEGL.so.1')
        p, i, u = C.c_void_p, C.c_int, C.c_uint
        display = bind(self.egl, 'eglGetPlatformDisplay', p, u, p, p)(0x31DD, None, None)
        major, minor = i(), i()
        assert bind(self.egl, 'eglInitialize', u, p, p, p)(display, C.byref(major), C.byref(minor)), 'EGL initialize failed'
        assert bind(self.egl, 'eglBindAPI', u, u)(0x30A2), 'OpenGL API unavailable'
        attrs = (i*7)(0x3033, 1, 0x3040, 8, 0x3024, 8, 0x3038)
        config, count = p(), i()
        assert bind(self.egl, 'eglChooseConfig', u, p, p, p, i, p)(display, attrs, C.byref(config), 1, C.byref(count)) and count.value
        ctxattrs = (i*7)(0x3098, 4, 0x30FB, 5, 0x30FD, 1, 0x3038)
        context = bind(self.egl, 'eglCreateContext', p, p, p, p, p)(display, config, None, ctxattrs)
        assert context, 'OpenGL 4.5 context unavailable'
        assert bind(self.egl, 'eglMakeCurrent', u, p, p, p, p)(display, None, None, context)
        self.display, self.context = display, context
        self.programs = OrderedDict()
        self.program_limit=64
        self.closed = False
        self.gl = C.CDLL('libGL.so.1')
        signatures = {
            'DeleteShader': (None,u), 'DeleteProgram': (None,u),
            'DeleteTextures': (None,i,p), 'DeleteBuffers': (None,i,p),
            'DeleteFramebuffers': (None,i,p), 'DeleteVertexArrays': (None,i,p),
            'GetString': (C.c_char_p, u), 'CreateShader': (u,u),
            'ShaderSource': (None,u,i,p,p), 'CompileShader': (None,u),
            'GetShaderiv': (None,u,u,p), 'GetShaderInfoLog': (None,u,i,p,p),
            'CreateProgram': (u,), 'AttachShader': (None,u,u), 'LinkProgram': (None,u),
            'GetProgramiv': (None,u,u,p), 'GetProgramInfoLog': (None,u,i,p,p),
            'UseProgram': (None,u), 'GenVertexArrays': (None,i,p), 'BindVertexArray': (None,u),
            'GenTextures': (None,i,p), 'BindTexture': (None,u,u), 'ActiveTexture': (None,u),
            'TexParameteri': (None,u,u,i), 'TexImage2D': (None,u,i,i,i,i,i,u,u,p),
            'GenFramebuffers': (None,i,p), 'BindFramebuffer': (None,u,u),
            'FramebufferTexture2D': (None,u,u,u,u,i), 'CheckFramebufferStatus': (u,u),
            'GenBuffers': (None,i,p), 'BindBuffer': (None,u,u), 'BufferData': (None,u,C.c_ssize_t,p,u),
            'BindBufferBase': (None,u,u,u), 'Viewport': (None,i,i,i,i),
            'ClearColor': (None,C.c_float,C.c_float,C.c_float,C.c_float), 'Clear': (None,u),
            'DrawArrays': (None,u,i,i), 'ReadPixels': (None,i,i,i,i,u,u,p), 'GetError': (u,),
        }
        for name, sig in signatures.items():
            setattr(self, name, bind(self.gl, 'gl'+name, *sig))
        self.renderer = self.GetString(0x1F01).decode()
        vao = u(); self.GenVertexArrays(1,C.byref(vao)); self.BindVertexArray(vao)
        self.target = self.texture(np.zeros((self.height,self.width,4),np.float32), 0)
        fbo = u(); self.GenFramebuffers(1,C.byref(fbo)); self.BindFramebuffer(0x8D40,fbo)
        self.FramebufferTexture2D(0x8D40,0x8CE0,0x0DE1,self.target,0)
        assert self.CheckFramebufferStatus(0x8D40)==0x8CD5
        self.vao,self.fbo=vao.value,fbo.value
        self.Viewport(0,0,self.width,self.height)

    def texture(self, array, unit, mipmaps=None, wrap="clamp", filters=("linear","linear","linear")):
        modes={"clamp":0x812F,"repeat":0x2901,"mirror":0x8370,"zero":0x812D}
        if wrap not in modes:raise ValueError("unknown texture wrap mode")
        if len(filters)!=3 or filters[0] not in ("nearest","linear") or filters[1] not in ("nearest","linear") or filters[2] not in ("none","nearest","linear"):
            raise ValueError("unsupported texture filters")
        min_filter,mag_filter,mip_filter=filters
        base={"nearest":0x2600,"linear":0x2601}
        array = np.ascontiguousarray(array,dtype=np.float32)
        name=C.c_uint(); self.GenTextures(1,C.byref(name)); self.ActiveTexture(0x84C0+unit)
        self.BindTexture(0x0DE1,name)
        for key,value in [(0x2801,base[min_filter]),(0x2800,base[mag_filter]),(0x2802,modes[wrap]),(0x2803,modes[wrap]),(0x813D,0)]:
            self.TexParameteri(0x0DE1,key,value)
        self.TexImage2D(0x0DE1,0,0x8814,array.shape[1],array.shape[0],0,0x1908,0x1406,array.ctypes.data)
        if mipmaps:
            mip_modes={("nearest","nearest"):0x2700,("linear","nearest"):0x2701,
                       ("nearest","linear"):0x2702,("linear","linear"):0x2703}
            self.TexParameteri(0x0DE1,0x2801,base[min_filter] if mip_filter=="none" else mip_modes[min_filter,mip_filter])
            self.TexParameteri(0x0DE1,0x813D,len(mipmaps))
            for level, pixels in enumerate(mipmaps,1):
                pixels=np.ascontiguousarray(pixels,dtype=np.float32)
                assert pixels.shape==(max(1,array.shape[0]>>level),max(1,array.shape[1]>>level),4)
                self.TexImage2D(0x0DE1,level,0x8814,pixels.shape[1],pixels.shape[0],0,0x1908,0x1406,pixels.ctypes.data)
        return name.value

    def shader(self, kind, source):
        shader=self.CreateShader(kind); text=C.c_char_p(source.encode())
        self.ShaderSource(shader,1,C.byref(text),None); self.CompileShader(shader)
        ok=C.c_int(); self.GetShaderiv(shader,0x8B81,C.byref(ok))
        if not ok.value:
            log=C.create_string_buffer(8192);self.GetShaderInfoLog(shader,8192,None,log)
            self.DeleteShader(shader)
            raise RuntimeError(log.value.decode())
        return shader

    def render(self, filename, source, sdf, uniforms, extension=None, edr=1.0, mipmaps=None, vertex_source=None, noise_texture=None, wraps=("clamp","clamp","clamp"), filters=(("linear","linear","linear"),)*3,extra_uniforms=None,vertex_count=3,fragment_source=None):
        if type(vertex_count)is not int or vertex_count not in(3,6):raise ValueError('triangle or two-triangle quad vertex count required')
        if len(wraps)!=3 or any(v not in ("clamp","repeat","mirror","zero") for v in wraps):raise ValueError("three supported wrap modes required")
        if len(filters)!=3:raise ValueError("three texture filter profiles required")
        extra_uniforms={} if extra_uniforms is None else extra_uniforms
        if any(type(k) is not int or k in (1,6,9) or not 0<=k<16 for k in extra_uniforms):raise ValueError('unsupported extra uniform binding')
        vertex='''#version 450
layout(location=0) out vec2 sourceUV;
layout(location=1) out vec2 sdfUV;
void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);
sourceUV=p;sdfUV=p;gl_Position=vec4(p*2.0-1.0,0,1);}
'''
        if self.closed: raise RuntimeError('Renderer is closed')
        p=C.c_void_p
        assert bind(self.egl,'eglMakeCurrent',C.c_uint,p,p,p,p)(self.display,None,None,self.context)
        self.BindFramebuffer(0x8D40,self.fbo);self.BindVertexArray(self.vao)
        fragment=(HERE/filename).read_text() if fragment_source is None else fragment_source
        key=(vertex_source or vertex,fragment)
        program=self.programs.get(key)
        if program is None:
            program=self.CreateProgram();shaders=[]
            try:
                for kind,code in [(0x8B31,key[0]),(0x8B30,key[1])]:
                    shader=self.shader(kind,code);shaders.append(shader);self.AttachShader(program,shader)
                self.LinkProgram(program);ok=C.c_int();self.GetProgramiv(program,0x8B82,C.byref(ok))
                if not ok.value:
                    log=C.create_string_buffer(8192);self.GetProgramInfoLog(program,8192,None,log)
                    raise RuntimeError(log.value.decode())
                self.programs[key]=program
                while len(self.programs)>self.program_limit:
                    _,old=self.programs.popitem(last=False);self.DeleteProgram(old)
            except BaseException:
                self.DeleteProgram(program);raise
            finally:
                for shader in shaders:self.DeleteShader(shader)
        self.programs.move_to_end(key)
        self.UseProgram(program)
        textures=[];buffers=[]
        try:
            textures.append(self.texture(source,3,mipmaps,wraps[0],filters[0]));textures.append(self.texture(sdf,4,wrap=wraps[1],filters=filters[1]))
            if noise_texture is not None:textures.append(self.texture(noise_texture,5,wrap=wraps[2],filters=filters[2]))
            for binding,data in [(1,uniforms),(6,struct.pack('<4f',edr,0,0,0)),(9,extension or bytes(32)),*extra_uniforms.items()]:
                name=C.c_uint();self.GenBuffers(1,C.byref(name));buffers.append(name.value);self.BindBuffer(0x8A11,name)
                blob=C.create_string_buffer(bytes(data));self.BufferData(0x8A11,len(data),blob,0x88E4)
                self.BindBufferBase(0x8A11,binding,name)
            self.ClearColor(0,0,0,0);self.Clear(0x4000);self.DrawArrays(4,0,vertex_count)
            result=np.empty((self.height,self.width,4),np.float32)
            self.ReadPixels(0,0,self.width,self.height,0x1908,0x1406,result.ctypes.data)
            assert self.GetError()==0,'OpenGL error'
            return result
        finally:
            for name in textures:self.DeleteTextures(1,C.byref(C.c_uint(name)))
            for name in buffers:self.DeleteBuffers(1,C.byref(C.c_uint(name)))

    def close(self):
        if self.closed:return
        p=C.c_void_p;u=C.c_uint
        bind(self.egl,'eglMakeCurrent',u,p,p,p,p)(self.display,None,None,self.context)
        self.UseProgram(0)
        for program in self.programs.values():self.DeleteProgram(program)
        self.DeleteTextures(1,C.byref(u(self.target)))
        self.DeleteFramebuffers(1,C.byref(u(self.fbo)))
        self.DeleteVertexArrays(1,C.byref(u(self.vao)))
        bind(self.egl,'eglMakeCurrent',u,p,p,p,p)(self.display,None,None,None)
        bind(self.egl,'eglDestroyContext',u,p,p)(self.display,self.context)
        # EGL display may be shared by other instances; do not terminate it here.
        self.closed=True

    def __enter__(self):return self
    def __exit__(self,*_):self.close()



def put(buffer, offset, *values):
    struct.pack_into('<'+'f'*len(values),buffer,offset,*values)


def main(out):
    out.mkdir(parents=True,exist_ok=True)
    r=Renderer(); n=r.size
    source=np.empty((n,n,4),np.float32);source[:]=(.2,.4,.6,1)
    sdf=np.zeros_like(source);sdf[:]=(-10,1,0,1)
    fg=bytearray(80);put(fg,0,1,0,0,1);put(fg,48,0,1,0,0);put(fg,64,1,0)
    bg=bytearray(320);put(bg,0,1,0,0,1);put(bg,176,-2,-1,0,1);put(bg,32,-1,1)
    checks={}
    for shader,uniforms in [('foreground.frag',fg),('background-minimal.frag',bg),('background-refraction.frag',bg)]:
        result=r.render(shader,source,sdf,uniforms)
        error=float(np.max(np.abs(result-source)))
        assert error<2e-6,(shader,error)
        checks[shader+'/constant']=error
    # Premultiplied foreground alpha must survive the unpremultiply/reweight path.
    transparent=source.copy();transparent[:]=(.1,.2,.3,.5)
    result=r.render('foreground.frag',transparent,sdf,fg,edr=2)
    assert np.allclose(result,(.2,.4,.6,.5),atol=2e-6)
    sdf[:,:,3]=0
    assert np.max(np.abs(r.render('foreground.frag',source,sdf,fg)))==0
    checks['foreground/alpha-edr-and-empty']='PASS'
    # A linear UV ramp gives an independent analytic displacement oracle.
    # Non-axis-aligned gradient catches swapped components and matrix errors.
    y,x=np.mgrid[:n,:n]
    ramp=np.empty_like(source)
    ramp[:,:,0]=(x+.5)/n;ramp[:,:,1]=(y+.5)/n
    ramp[:,:,2]=.3;ramp[:,:,3]=1
    plane=np.empty_like(sdf);plane[:]=(-.5,.6,.8,1)
    displacement=8*(1-np.sqrt(.75))
    expected=ramp.copy()
    expected[:,:,0]+=displacement*.6/n
    expected[:,:,1]+=displacement*.8/n
    for shader,uniforms in [('foreground.frag',fg),('background-refraction.frag',bg)]:
        probe=bytearray(uniforms)
        put(probe,0,1/n,0,0,1/n);put(probe,16,8,1)
        result=r.render(shader,ramp,plane,probe)
        error=float(np.max(np.abs(result[8:-8,8:-8]-expected[8:-8,8:-8])))
        assert error<2e-6,(shader,'refraction ramp',error)
        checks[shader+'/analytic-refraction-ramp']=error
    # Synthetic geometry: raw distance in pixels, unit gradient, alpha 1.
    y,x=np.mgrid[:n,:n];dx=x+.5-n/2;dy=y+.5-n/2;length=np.hypot(dx,dy)
    sdf[:,:,0]=length-40;sdf[:,:,1]=dx/np.maximum(length,1e-6)
    sdf[:,:,2]=dy/np.maximum(length,1e-6);sdf[:,:,3]=1
    source[:,:,0]=(x%16)/15;source[:,:,1]=(y%16)/15;source[:,:,2]=.3;source[:,:,3]=1
    put(fg,0,1/n,0,0,1/n);put(fg,16,8,.1,0,4,.1,0);put(fg,40,1,0)
    put(bg,0,1/n,0,0,1/n);put(bg,16,8,.1,3,.2);put(bg,224,.5)
    images=[]
    for shader,uniforms in [('background-refraction.frag',bg),('foreground.frag',fg)]:
        result=r.render(shader,source,sdf,uniforms)
        assert np.isfinite(result).all()
        np.save(out/(shader+'.npy'),result)
        alpha=np.clip(result[:,:,3:4],0,1)
        display=np.clip(result[:,:,:3]+.12*(1-alpha),0,1)
        images.append(Image.fromarray(np.uint8(display*255)))
    combined=Image.new('RGB',(n*2,n));combined.paste(images[0],(0,0));combined.paste(images[1],(n,0))
    combined.save(out/'synthetic-shader-fixture.png')
    report={'renderer':r.renderer,'checks':checks,'fixture':'synthetic independent outputs, background left / foreground right; not final compositing or Apple defaults'}
    (out/'render-validation.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))


if __name__=='__main__':
    main(Path(sys.argv[1]).resolve())
