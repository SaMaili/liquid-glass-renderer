//! Synchronous, thread-confined Linux render-pass core for the recovered GLSL.
//! The application supplies scene order and material inputs. No frame scheduler.
mod gl;
pub mod interaction;
pub mod material;
pub mod overlay;
pub mod raster;
pub mod scene;
pub mod frame;
pub mod shape;
use gl::*;
use std::{collections::VecDeque,ffi::{CStr,CString},ptr,rc::Rc};

const GL_TIME_ELAPSED: u32 = 0x88bf;
const GL_QUERY_COUNTER_BITS: u32 = 0x8864;
const GL_QUERY_RESULT: u32 = 0x8866;

pub type Result<T> = std::result::Result<T,String>;
pub const DEFAULT_VERTEX:&str = "#version 450\nlayout(location=0) out vec2 sourceUV;\nlayout(location=1) out vec2 sdfUV;\nvoid main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);\nsourceUV=p;sdfUV=p;gl_Position=vec4(p*2.0-1.0,0,1);}\n";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Size { pub width:u32,pub height:u32 }
impl Size {
    pub fn new(width:u32,height:u32)->Result<Self>{
        if !(1..=4096).contains(&width)||!(1..=4096).contains(&height){return Err("dimensions must be 1..4096".into());}
        Ok(Self{width,height})
    }
    pub fn pixels(self)->usize{self.width as usize*self.height as usize}
    fn validate(self)->Result<()>{Self::new(self.width,self.height).map(|_|())}
}
#[derive(Clone,Copy)]
pub struct Raster<'a>{pub size:Size,pub rgba:&'a [f32]}
impl Raster<'_>{
    fn validate(&self)->Result<()>{self.size.validate()?;if self.rgba.len()!=self.size.pixels()*4||!self.rgba.iter().all(|v|v.is_finite()){return Err("finite RGBA raster with matching dimensions required".into());}Ok(())}
}
#[derive(Clone,Copy,Debug)]
pub enum Wrap{Clamp,Repeat,Mirror,Zero}
#[derive(Clone,Copy,Debug)]
pub enum Filter{Nearest,Linear}
#[derive(Clone,Copy,Debug)]
pub enum MipFilter{None,Nearest,Linear}
#[derive(Clone,Copy,Debug)]
pub struct Sampler{pub wrap:Wrap,pub min:Filter,pub mag:Filter,pub mip:MipFilter}
impl Default for Sampler{fn default()->Self{Self{wrap:Wrap::Clamp,min:Filter::Linear,mag:Filter::Linear,mip:MipFilter::Linear}}}
pub struct TextureInput<'a>{pub image:Raster<'a>,pub mipmaps:&'a [Raster<'a>],pub sampler:Sampler}
pub struct Pass<'a>{
    pub fragment:&'a str,
    pub vertex:Option<&'a str>,
    pub source:TextureInput<'a>,
    pub sdf:TextureInput<'a>,
    pub noise:Option<TextureInput<'a>>,
    pub uniforms:&'a [u8],
    pub extension:&'a [u8],
    pub edr:f32,
    pub extra_uniforms:&'a [(u32,&'a [u8])],
    pub vertex_count:u32,
}
#[derive(Default,Debug,Clone,Copy)]
pub struct Stats{pub draws:u64,pub texture_allocations:u64,pub texture_pool_hits:u64,pub texture_evictions:u64,pub buffer_allocations:u64,pub program_compilations:u64,pub program_hits:u64,pub resident_texture_bytes:usize,pub resident_buffer_bytes:usize}
struct Texture{name:u32,levels:Vec<Size>}
impl Texture{fn bytes(&self)->usize{self.levels.iter().map(|s|s.pixels()*16).sum()}}
struct Buffer{name:u32,bytes:usize}
struct Program{name:u32,vertex:String,fragment:String}

/// Owns all GL objects. Readback completes each submission before reuse/drop.
/// Explicit pass calls always draw. Scene reuse and content updates are
/// available through `frame::FrameCache` and `scene::LiveScene`.
pub struct Renderer{
    display:Ptr,context:Ptr,size:Size,vao:u32,fbo:u32,target:Texture,
    textures:[Option<Texture>;3],spare_textures:VecDeque<Texture>,buffers:[Option<Buffer>;16],programs:Vec<Program>,
    stats:Stats,budget:usize,identity:Rc<()>,
    timer_query: u32,
    draw_time_ns: u64,
}
impl Renderer{
    pub fn new(size:Size)->Result<Self>{Self::with_budget(size,256*1024*1024)}
    pub fn with_budget(size:Size,budget:usize)->Result<Self>{
        size.validate()?;if size.pixels()*16>budget{return Err("target exceeds texture budget".into());}
        unsafe{
            let display=eglGetPlatformDisplay(0x31dd,ptr::null_mut(),ptr::null());
            let(mut major,mut minor)=(0,0);
            if display.is_null()||eglInitialize(display,&mut major,&mut minor)==0{return Err("EGL initialize failed".into());}
            if eglBindAPI(0x30a2)==0{return Err("OpenGL API unavailable".into());}
            let attrs=[0x3033,1,0x3040,8,0x3024,8,0x3038];let mut config=ptr::null_mut();let mut count=0;
            if eglChooseConfig(display,attrs.as_ptr(),&mut config,1,&mut count)==0||count==0{return Err("EGL config unavailable".into());}
            let attrs=[0x3098,4,0x30fb,5,0x30fd,1,0x3038];
            let context=eglCreateContext(display,config,ptr::null_mut(),attrs.as_ptr());
            if context.is_null(){return Err("OpenGL 4.5 core unavailable".into());}
            if eglMakeCurrent(display,ptr::null_mut(),ptr::null_mut(),context)==0{eglDestroyContext(display,context);return Err("EGL make current failed".into());}
            let mut r=Self{display,context,size,vao:0,fbo:0,target:Texture{name:0,levels:Vec::new()},textures:[None,None,None],spare_textures:VecDeque::new(),buffers:std::array::from_fn(|_|None),programs:Vec::new(),stats:Stats::default(),budget,identity:Rc::new(()),timer_query:0,draw_time_ns:0};
            glGenVertexArrays(1,&mut r.vao);glBindVertexArray(r.vao);
            glGenTextures(1,&mut r.target.name);glBindTexture(0x0de1,r.target.name);
            glTexParameteri(0x0de1,0x2801,0x2601);glTexParameteri(0x0de1,0x2800,0x2601);glTexParameteri(0x0de1,0x813d,0);
            glTexImage2D(0x0de1,0,0x8814,size.width as i32,size.height as i32,0,0x1908,0x1406,ptr::null());
            r.target.levels.push(size);r.stats.texture_allocations=1;r.stats.resident_texture_bytes=size.pixels()*16;
            glGenFramebuffers(1,&mut r.fbo);glBindFramebuffer(0x8d40,r.fbo);glFramebufferTexture2D(0x8d40,0x8ce0,0x0de1,r.target.name,0);
            if glCheckFramebufferStatus(0x8d40)!=0x8cd5{return Err("incomplete framebuffer".into());}
            r.check()?;Ok(r)
        }
    }
    fn current(&self)->Result<()>{unsafe{if eglMakeCurrent(self.display,ptr::null_mut(),ptr::null_mut(),self.context)==0{Err("EGL make current failed".into())}else{Ok(())}}}
    fn check(&self)->Result<()>{let error=unsafe{glGetError()};if error!=0{Err(format!("OpenGL error {error:#x}"))}else{Ok(())}}
    pub fn device(&self)->Result<String>{self.current()?;unsafe{let name=glGetString(0x1f01);if name.is_null(){return Err("GL renderer name unavailable".into());}Ok(CStr::from_ptr(name).to_string_lossy().into_owned())}}
    pub fn stats(&self)->Stats{self.stats}
    /// Enable GL elapsed-time queries for clear and draw, excluding readback.
    /// This is opt-in instrumentation and can affect submission timing.
    pub fn enable_draw_timing(&mut self) -> Result<()> {
        if self.timer_query == 0 {
            self.current()?;
            let mut bits = 0;
            unsafe { glGetQueryiv(GL_TIME_ELAPSED, GL_QUERY_COUNTER_BITS, &mut bits); }
            self.check()?;
            if bits == 0 {
                return Err("OpenGL elapsed-time queries unavailable".into());
            }
            unsafe { glGenQueries(1, &mut self.timer_query); }
            self.check()?;
        }
        Ok(())
    }

    /// Sum of the GL draw intervals recorded since timing was enabled.
    pub fn draw_time_ns(&self) -> u64 { self.draw_time_ns }

    pub fn size(&self)->Size{self.size}
    fn program(&mut self,vertex:&str,fragment:&str)->Result<u32>{
        if let Some(i)=self.programs.iter().position(|p|p.vertex==vertex&&p.fragment==fragment){let p=self.programs.remove(i);let name=p.name;self.programs.push(p);self.stats.program_hits+=1;return Ok(name);}
        unsafe{
            let program=glCreateProgram();let mut shaders=Vec::new();
            let result=(||{
                for(kind,source)in [(0x8b31,vertex),(0x8b30,fragment)]{
                    let text=CString::new(source).map_err(|_|"shader contains NUL")?;
                    let shader=glCreateShader(kind);shaders.push(shader);glShaderSource(shader,1,&text.as_ptr(),ptr::null());glCompileShader(shader);
                    let mut ok=0;glGetShaderiv(shader,0x8b81,&mut ok);
                    if ok==0{let mut log=vec![0u8;16384];glGetShaderInfoLog(shader,log.len() as i32,ptr::null_mut(),log.as_mut_ptr().cast());return Err(String::from_utf8_lossy(&log).trim_end_matches('\0').to_string());}
                    glAttachShader(program,shader);
                }
                glLinkProgram(program);let mut ok=0;glGetProgramiv(program,0x8b82,&mut ok);
                if ok==0{let mut log=vec![0u8;16384];glGetProgramInfoLog(program,log.len() as i32,ptr::null_mut(),log.as_mut_ptr().cast());return Err(String::from_utf8_lossy(&log).trim_end_matches('\0').to_string());}Ok(())
            })();
            for shader in shaders{glDeleteShader(shader);}
            if let Err(error)=result{glDeleteProgram(program);return Err(error);}
            if self.programs.len()==64{let old=self.programs.remove(0);glDeleteProgram(old.name);}
            self.programs.push(Program{name:program,vertex:vertex.into(),fragment:fragment.into()});self.stats.program_compilations+=1;Ok(program)
        }
    }
    fn prepare_textures(&mut self, plans: &[Option<Vec<Size>>;3]) {
        // Readback synchronizes the previous pass. The current pass reserves
        // all matching storage before budget eviction.
        for (slot,plan) in plans.iter().enumerate() {
            if self.textures[slot].as_ref().is_some_and(|texture| Some(&texture.levels)!=plan.as_ref()) {
                self.spare_textures.push_back(self.textures[slot].take().unwrap());
            }
        }

        let mut new_bytes = 0;
        for (slot,plan) in plans.iter().enumerate() {
            let Some(levels) = plan else { continue };
            if self.textures[slot].is_some() { continue; }
            if let Some(index) = self.spare_textures.iter().rposition(|texture| &texture.levels==levels) {
                self.textures[slot] = self.spare_textures.remove(index);
                self.stats.texture_pool_hits += 1;
            } else {
                new_bytes += levels.iter().map(|size| size.pixels()*16).sum::<usize>();
            }
        }

        const MAX_SPARE_TEXTURES: usize = 32;
        while self.stats.resident_texture_bytes+new_bytes>self.budget
            || self.spare_textures.len()>MAX_SPARE_TEXTURES
        {
            let texture = self.spare_textures.pop_front().expect("validated active storage fits the budget");
            self.stats.resident_texture_bytes -= texture.bytes();
            self.stats.texture_evictions += 1;
            unsafe { glDeleteTextures(1,&texture.name); }
        }
    }

    fn upload_texture(&mut self,slot:usize,input:&TextureInput<'_>,levels:Vec<Size>) {
        let needs_allocation = self.textures[slot].is_none();
        unsafe{
            glActiveTexture(0x84c3+slot as u32);
            if needs_allocation{
                let mut name=0;glGenTextures(1,&mut name);self.textures[slot]=Some(Texture{name,levels});
                self.stats.texture_allocations+=1;self.stats.resident_texture_bytes+=self.textures[slot].as_ref().unwrap().bytes();
            }
            glBindTexture(0x0de1,self.textures[slot].as_ref().unwrap().name);
            let s=input.sampler;let filter=|v|match v{Filter::Nearest=>0x2600,Filter::Linear=>0x2601};
            let min=if input.mipmaps.is_empty(){filter(s.min)}else{match(s.min,s.mip){(_,MipFilter::None)=>filter(s.min),(Filter::Nearest,MipFilter::Nearest)=>0x2700,(Filter::Linear,MipFilter::Nearest)=>0x2701,(Filter::Nearest,MipFilter::Linear)=>0x2702,(Filter::Linear,MipFilter::Linear)=>0x2703}};
            let wrap=match s.wrap{Wrap::Clamp=>0x812f,Wrap::Repeat=>0x2901,Wrap::Mirror=>0x8370,Wrap::Zero=>0x812d};
            for(param,value)in [(0x2801,min),(0x2800,filter(s.mag)),(0x2802,wrap),(0x2803,wrap),(0x813d,input.mipmaps.len() as i32)]{glTexParameteri(0x0de1,param,value);}
            for(level,image)in std::iter::once(&input.image).chain(input.mipmaps.iter()).enumerate(){
                let Size{width,height}=image.size;
                if needs_allocation{glTexImage2D(0x0de1,level as i32,0x8814,width as i32,height as i32,0,0x1908,0x1406,image.rgba.as_ptr().cast());}
                else{glTexSubImage2D(0x0de1,level as i32,0,0,width as i32,height as i32,0x1908,0x1406,image.rgba.as_ptr().cast());}
            }
        }
    }
    fn buffer(&mut self,binding:u32,data:&[u8]){
        let slot=binding as usize;
        unsafe{
            if self.buffers[slot].is_none(){let mut name=0;glGenBuffers(1,&mut name);self.buffers[slot]=Some(Buffer{name,bytes:0});}
            let buffer=self.buffers[slot].as_mut().unwrap();glBindBuffer(0x8a11,buffer.name);
            if buffer.bytes<data.len(){self.stats.resident_buffer_bytes=self.stats.resident_buffer_bytes-buffer.bytes+data.len();buffer.bytes=data.len();self.stats.buffer_allocations+=1;glBufferData(0x8a11,data.len() as isize,data.as_ptr().cast(),0x88e4);}
            else{glBufferSubData(0x8a11,0,data.len() as isize,data.as_ptr().cast());}
            glBindBufferRange(0x8a11,binding,buffer.name,0,data.len() as isize);
        }
    }
    pub fn render(&mut self,pass:&Pass<'_>)->Result<Vec<f32>>{
        self.render_region(pass, self.size)
    }

    // Smaller mip outputs use a smaller viewport in the same owned target.
    fn render_region(&mut self, pass: &Pass<'_>, output_size: Size) -> Result<Vec<f32>> {
        self.render_rect(pass, output_size, raster::PixelRect::full(output_size))
    }

    // Crop the draw/readback while retaining full-viewport UVs and derivatives.
    fn render_rect(&mut self, pass: &Pass<'_>, output_size: Size, rect: raster::PixelRect) -> Result<Vec<f32>> {
        output_size.validate()?;
        rect.check(output_size)?;
        if output_size.width > self.size.width || output_size.height > self.size.height {
            return Err("output viewport exceeds render target".into());
        }
        if !matches!(pass.vertex_count,3|6)||!pass.edr.is_finite(){return Err("finite EDR and triangle/quad vertex count required".into());}
        if pass.fragment.len()>1024*1024||pass.vertex.is_some_and(|v|v.len()>1024*1024){return Err("shader exceeds source limit".into());}
        let mut required=self.target.bytes();
        for image in [&pass.source,&pass.sdf].into_iter().chain(pass.noise.as_ref()){
            image.image.validate()?;required+=image.image.size.pixels()*16;
            let mut previous=image.image.size;
            for level in image.mipmaps{
                level.validate()?;let expected=Size{width:(previous.width/2).max(1),height:(previous.height/2).max(1)};
                if previous.width==1&&previous.height==1||level.size!=expected{return Err("invalid mip chain".into());}
                previous=level.size;required+=level.size.pixels()*16;
            }
        }
        if required>self.budget{return Err("pass exceeds texture budget".into());}
        let mut used=[false;16];used[1]=true;used[6]=true;used[9]=true;
        for(binding,data)in pass.extra_uniforms{
            if *binding>=16||used[*binding as usize]||data.is_empty()||data.len()>65536{return Err("invalid or duplicate uniform binding".into());}used[*binding as usize]=true;
        }
        if pass.uniforms.is_empty()||pass.uniforms.len()>65536||pass.extension.len()>32{return Err("uniform size outside pass contract".into());}
        self.current()?;
        let program=self.program(pass.vertex.unwrap_or(DEFAULT_VERTEX),pass.fragment)?;
        let inputs = [Some(&pass.source),Some(&pass.sdf),pass.noise.as_ref()];
        let plans = inputs.map(|input| input.map(|input| {
            std::iter::once(input.image.size).chain(input.mipmaps.iter().map(|r|r.size)).collect()
        }));
        self.prepare_textures(&plans);
        unsafe{glBindFramebuffer(0x8d40,self.fbo);glBindVertexArray(self.vao);glViewport(0,0,output_size.width as i32,output_size.height as i32);glUseProgram(program);}
        for (slot,(input,plan)) in inputs.into_iter().zip(plans).enumerate() {
            if let (Some(input),Some(plan)) = (input,plan) {
                self.upload_texture(slot,input,plan);
            } else {
                unsafe { glActiveTexture(0x84c3+slot as u32); glBindTexture(0x0de1,0); }
            }
        }
        if let Err(error) = self.check() {
            // An incomplete GL allocation cannot be returned to the pool.
            for texture in self.textures.iter_mut().filter_map(Option::take) {
                self.stats.resident_texture_bytes -= texture.bytes();
                unsafe { glDeleteTextures(1,&texture.name); }
            }
            return Err(error);
        }
        // Clear unused bindings so a previous pass cannot supply stale uniforms.
        for(binding,present)in used.iter().enumerate(){if !present{unsafe{glBindBufferBase(0x8a11,binding as u32,0);}}}
        self.buffer(1,pass.uniforms);
        let mut edr=[0u8;16];edr[..4].copy_from_slice(&pass.edr.to_ne_bytes());self.buffer(6,&edr);
        let mut extension=[0u8;32];extension[..pass.extension.len()].copy_from_slice(pass.extension);self.buffer(9,&extension);
        for(binding,data)in pass.extra_uniforms{self.buffer(*binding,data);}
        if let Err(error) = self.check() {
            for buffer in self.buffers.iter_mut().filter_map(Option::take) {
                self.stats.resident_buffer_bytes -= buffer.bytes;
                unsafe { glDeleteBuffers(1,&buffer.name); }
            }
            return Err(error);
        }
        let mut output=vec![0.0f32;rect.width as usize * rect.height as usize * 4];
        let (x,y,w,h)=(rect.x as i32,rect.y as i32,rect.width as i32,rect.height as i32);
        let clipped=rect!=raster::PixelRect::full(output_size);
        unsafe{
            if clipped { glEnable(0x0c11); glScissor(x,y,w,h); }
            if self.timer_query != 0 { glBeginQuery(GL_TIME_ELAPSED, self.timer_query); }
            glClearColor(0.,0.,0.,0.);
            glClear(0x4000);
            glDrawArrays(4,0,pass.vertex_count as i32);
            if self.timer_query != 0 { glEndQuery(GL_TIME_ELAPSED); }
            glReadPixels(x,y,w,h,0x1908,0x1406,output.as_mut_ptr().cast());
            if self.timer_query != 0 {
                let mut elapsed = 0;
                glGetQueryObjectui64v(self.timer_query, GL_QUERY_RESULT, &mut elapsed);
                self.draw_time_ns = self.draw_time_ns.saturating_add(elapsed);
            }
            if clipped { glDisable(0x0c11); }
        }
        self.check()?;self.stats.draws+=1;
        if !output.iter().all(|v|v.is_finite()){return Err("nonfinite render output".into());}
        Ok(output)
    }
}
impl Drop for Renderer{
    fn drop(&mut self){unsafe{
        if self.current().is_ok(){
            if self.timer_query != 0 { glDeleteQueries(1, &self.timer_query); }
            glUseProgram(0);for p in &self.programs{glDeleteProgram(p.name);}
            for t in self.textures.iter().flatten().chain(&self.spare_textures).chain(std::iter::once(&self.target)){glDeleteTextures(1,&t.name);}
            for b in self.buffers.iter().flatten(){glDeleteBuffers(1,&b.name);}
            glDeleteFramebuffers(1,&self.fbo);glDeleteVertexArrays(1,&self.vao);
            eglMakeCurrent(self.display,ptr::null_mut(),ptr::null_mut(),ptr::null_mut());
        }
        eglDestroyContext(self.display,self.context);
        // EGL displays can be shared with other renderer owners: no terminate.
    }}
}
