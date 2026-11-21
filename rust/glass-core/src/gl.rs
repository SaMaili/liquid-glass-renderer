//! Private Linux EGL/OpenGL ABI. All public inputs are validated in lib.rs.
#![allow(non_snake_case)]
use std::ffi::{c_char,c_void};
pub type Ptr = *mut c_void;
#[link(name="EGL")]
unsafe extern "C" {
    pub fn eglGetPlatformDisplay(platform:u32,native:Ptr,attrs:*const isize)->Ptr;
    pub fn eglInitialize(display:Ptr,major:*mut i32,minor:*mut i32)->u32;
    pub fn eglBindAPI(api:u32)->u32;
    pub fn eglChooseConfig(display:Ptr,attrs:*const i32,config:*mut Ptr,count:i32,found:*mut i32)->u32;
    pub fn eglCreateContext(display:Ptr,config:Ptr,share:Ptr,attrs:*const i32)->Ptr;
    pub fn eglMakeCurrent(display:Ptr,draw:Ptr,read:Ptr,context:Ptr)->u32;
    pub fn eglDestroyContext(display:Ptr,context:Ptr)->u32;
}
#[link(name="GL")]
unsafe extern "C" {
    pub fn glGetString(name:u32)->*const c_char;
    pub fn glGetError()->u32;
    pub fn glCreateShader(kind:u32)->u32;
    pub fn glShaderSource(shader:u32,count:i32,strings:*const *const c_char,lengths:*const i32);
    pub fn glCompileShader(shader:u32);
    pub fn glGetShaderiv(shader:u32,param:u32,value:*mut i32);
    pub fn glGetShaderInfoLog(shader:u32,size:i32,length:*mut i32,log:*mut c_char);
    pub fn glDeleteShader(shader:u32);
    pub fn glCreateProgram()->u32;
    pub fn glAttachShader(program:u32,shader:u32);
    pub fn glLinkProgram(program:u32);
    pub fn glGetProgramiv(program:u32,param:u32,value:*mut i32);
    pub fn glGetProgramInfoLog(program:u32,size:i32,length:*mut i32,log:*mut c_char);
    pub fn glDeleteProgram(program:u32);
    pub fn glUseProgram(program:u32);
    pub fn glGenVertexArrays(count:i32,names:*mut u32);
    pub fn glBindVertexArray(name:u32);
    pub fn glDeleteVertexArrays(count:i32,names:*const u32);
    pub fn glGenTextures(count:i32,names:*mut u32);
    pub fn glBindTexture(target:u32,name:u32);
    pub fn glActiveTexture(unit:u32);
    pub fn glTexParameteri(target:u32,param:u32,value:i32);
    pub fn glTexImage2D(target:u32,level:i32,internal:i32,width:i32,height:i32,border:i32,format:u32,kind:u32,pixels:*const c_void);
    pub fn glTexSubImage2D(target:u32,level:i32,x:i32,y:i32,width:i32,height:i32,format:u32,kind:u32,pixels:*const c_void);
    pub fn glDeleteTextures(count:i32,names:*const u32);
    pub fn glGenFramebuffers(count:i32,names:*mut u32);
    pub fn glBindFramebuffer(target:u32,name:u32);
    pub fn glFramebufferTexture2D(target:u32,attachment:u32,texture_target:u32,name:u32,level:i32);
    pub fn glCheckFramebufferStatus(target:u32)->u32;
    pub fn glDeleteFramebuffers(count:i32,names:*const u32);
    pub fn glGenBuffers(count:i32,names:*mut u32);
    pub fn glBindBuffer(target:u32,name:u32);
    pub fn glBufferData(target:u32,size:isize,data:*const c_void,usage:u32);
    pub fn glBufferSubData(target:u32,offset:isize,size:isize,data:*const c_void);
    pub fn glBindBufferBase(target:u32,binding:u32,name:u32);
    pub fn glBindBufferRange(target:u32,binding:u32,name:u32,offset:isize,size:isize);
    pub fn glDeleteBuffers(count:i32,names:*const u32);
    pub fn glGenQueries(count:i32,names:*mut u32);
    pub fn glDeleteQueries(count:i32,names:*const u32);
    pub fn glGetQueryiv(target:u32,pname:u32,params:*mut i32);
    pub fn glBeginQuery(target:u32,name:u32);
    pub fn glEndQuery(target:u32);
    pub fn glGetQueryObjectui64v(name:u32,pname:u32,params:*mut u64);
    pub fn glViewport(x:i32,y:i32,width:i32,height:i32);
    pub fn glClearColor(r:f32,g:f32,b:f32,a:f32);
    pub fn glClear(mask:u32);
    pub fn glDrawArrays(mode:u32,first:i32,count:i32);
    pub fn glReadPixels(x:i32,y:i32,width:i32,height:i32,format:u32,kind:u32,out:*mut c_void);
    pub fn glScissor(x:i32,y:i32,width:i32,height:i32);
    pub fn glEnable(cap:u32);
    pub fn glDisable(cap:u32);
}
