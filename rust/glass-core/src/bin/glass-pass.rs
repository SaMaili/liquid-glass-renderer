//! Bounded binary test adapter. No Python is used by the renderer process.
use liquid_glass_core::*;
use std::io::{self,Cursor,Read,Write};
fn u32le(input:&mut impl Read)->io::Result<u32>{let mut bytes=[0;4];input.read_exact(&mut bytes)?;Ok(u32::from_le_bytes(bytes))}
fn blob(input:&mut Cursor<Vec<u8>>)->std::result::Result<Vec<u8>,Box<dyn std::error::Error>>{
    let length=u32le(input)? as usize;
    if length>input.get_ref().len().saturating_sub(input.position() as usize){return Err("truncated blob".into());}
    let mut bytes=vec![0;length];input.read_exact(&mut bytes)?;Ok(bytes)
}
struct Image{size:Size,rgba:Vec<f32>}
struct Texture{levels:Vec<Image>,sampler:Sampler}
impl Texture{
    fn rasters(&self)->Vec<Raster<'_>>{self.levels.iter().map(|i|Raster{size:i.size,rgba:&i.rgba}).collect()}
}
fn texture(input:&mut Cursor<Vec<u8>>)->std::result::Result<Texture,Box<dyn std::error::Error>>{
    let wrap=match u32le(input)?{0=>Wrap::Clamp,1=>Wrap::Repeat,2=>Wrap::Mirror,3=>Wrap::Zero,_=>return Err("invalid wrap".into())};
    let mut filter=||Ok::<_,Box<dyn std::error::Error>>(match u32le(input)?{0=>Filter::Nearest,1=>Filter::Linear,_=>return Err("invalid filter".into())});
    let min=filter()?;let mag=filter()?;
    let mip=match u32le(input)?{0=>MipFilter::None,1=>MipFilter::Nearest,2=>MipFilter::Linear,_=>return Err("invalid mip filter".into())};
    let count=u32le(input)?;if !(1..=13).contains(&count){return Err("invalid mip count".into());}
    let mut levels=Vec::new();
    for _ in 0..count{
        let size=Size::new(u32le(input)?,u32le(input)?)?;let raw=blob(input)?;
        if raw.len()!=size.pixels()*16{return Err("invalid raster bytes".into());}
        let rgba=raw.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();
        levels.push(Image{size,rgba});
    }
    Ok(Texture{levels,sampler:Sampler{wrap,min,mag,mip}})
}
fn main()->std::result::Result<(),Box<dyn std::error::Error>>{
    let mut stdin=io::stdin().lock();let mut stdout=io::stdout().lock();let mut renderer:Option<Renderer>=None;
    loop{
        let length=u32le(&mut stdin)?;if length==0{break;}
        if length>256*1024*1024{return Err("request exceeds 256 MiB".into());}
        let mut raw=vec![0;length as usize];stdin.read_exact(&mut raw)?;let mut input=Cursor::new(raw);
        let size=Size::new(u32le(&mut input)?,u32le(&mut input)?)?;
        let vertex_count=u32le(&mut input)?;let edr=f32::from_bits(u32le(&mut input)?);
        let vertex=String::from_utf8(blob(&mut input)?)?;let fragment=String::from_utf8(blob(&mut input)?)?;
        let uniforms=blob(&mut input)?;let extension=blob(&mut input)?;
        let count=u32le(&mut input)?;if count>13{return Err("too many uniforms".into());}
        let mut extra=Vec::new();for _ in 0..count{extra.push((u32le(&mut input)?,blob(&mut input)?));}
        let count=u32le(&mut input)?;if !(2..=3).contains(&count){return Err("two or three textures required".into());}
        let mut textures=Vec::new();for _ in 0..count{textures.push(texture(&mut input)?);}
        if input.position()!=input.get_ref().len() as u64{return Err("trailing request data".into());}
        if renderer.as_ref().is_none_or(|r|r.size()!=size){renderer=Some(Renderer::new(size)?);}
        let rasters:Vec<_>=textures.iter().map(Texture::rasters).collect();
        let tex=|i:usize|TextureInput{image:rasters[i][0],mipmaps:&rasters[i][1..],sampler:textures[i].sampler};
        let extra:Vec<_>=extra.iter().map(|(b,v)|(*b,v.as_slice())).collect();
        let pass=Pass{fragment:&fragment,vertex:if vertex.is_empty(){None}else{Some(&vertex)},source:tex(0),sdf:tex(1),noise:if count==3{Some(tex(2))}else{None},uniforms:&uniforms,extension:&extension,edr,extra_uniforms:&extra,vertex_count};
        let output=renderer.as_mut().unwrap().render(&pass)?;
        stdout.write_all(&((output.len()*4) as u32).to_le_bytes())?;
        let bytes:Vec<u8>=output.iter().flat_map(|v|v.to_le_bytes()).collect();stdout.write_all(&bytes)?;stdout.flush()?;
    }
    if let Some(r)=renderer{eprintln!("{} {:?}",r.device()?,r.stats());}
    Ok(())
}
