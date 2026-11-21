//! Recovered 26A434 automatic Flex/Glow targets for stationary glass bounds.
//! The controller module connects the scalar kernels to caller-driven events.
pub mod controller;
use crate::Result;
mod data{include!("interaction_data.rs");}
#[derive(Clone,Copy)]
pub enum Input{Cursor,Touch}
#[derive(Clone,Copy,Debug)]
pub struct Profile{pub values:[f64;18]}
impl Profile{
    pub fn automatic(size:[f64;2],input:Input)->Result<Self>{
        if !size.iter().all(|v|v.is_finite()&&*v>0.){return Err("positive finite bounds required".into());}
        let kind=match input{Input::Cursor=>0,Input::Touch=>1};
        let long=size[0].max(size[1]);let short=size[0].min(size[1]);
        if long<120.{return Ok(Self{values:data::COMPACT[kind]});}
        let low=data::LOWER[kind];let high=data::UPPER[kind];let denominator=high[0]-low[0];
        let short=((short-low[0])/denominator).clamp(0.,1.);let long=((long-low[0])/denominator).clamp(0.,1.);
        let mut values=low;
        for i in [1,2,3,14,15,16,17]{values[i]=low[i]+(high[i]-low[i])*long;}
        for i in [4,5,6]{values[i]=low[i]+(high[i]-low[i])*short;}
        Ok(Self{values})
    }
}
#[derive(Clone,Copy)]
pub struct Gesture{
    pub size:[f64;2],pub start:[f64;2],pub translation:[f64;2],pub input:Input,
    pub active:bool,pub lift:bool,pub flex:bool,pub reduce_highlighting:bool,
}
#[derive(Debug,PartialEq)]
pub struct Flex{pub scale:[f64;2],pub translation:[f64;2],pub spring:[f64;2]}
#[derive(Debug,PartialEq)]
pub struct Glow{pub opacity:f64,pub big_opacity:f64,pub scale:f64,pub diameter:f64,pub position:[f64;2]}
impl Gesture{
    fn profile(&self)->Result<Profile>{
        if !self.start.iter().chain(self.translation.iter()).all(|v|v.is_finite()){return Err("finite local gesture required".into());}
        Profile::automatic(self.size,self.input)
    }
    pub fn flex_target(&self)->Result<Flex>{
        let spec=self.profile()?.values;let [w,h]=self.size;let [tx,ty]=self.translation;
        let ix=(self.start[0]+tx).clamp(0.,w)-self.start[0];let iy=(self.start[1]+ty).clamp(0.,h)-self.start[1];
        let base=if self.active&&self.lift{(w.max(h)+spec[4])/w.max(h)}else{1.};
        let(mut sx,mut sy,mut dx,mut dy)=(base,base,0.,0.);let threshold=spec[5];
        if self.active&&self.flex{
            if tx!=0.{let a=if threshold>0.{(tx.abs()-ix.abs())/threshold+ix.abs()/(threshold*4.)}else{0.};sx=base+a;sy=base-a;dx=w*a*if tx>=0.{1.}else{-1.};}
            if ty!=0.{let b=if threshold>0.{(ty.abs()-iy.abs())/threshold+iy.abs()/(threshold*4.)}else{0.};sx-=b;sy+=b;dy=h*b*if ty>=0.{1.}else{-1.};}
        }
        let result=Flex{scale:[sx,sy],translation:[dx,dy],spring:[spec[14],spec[15]]};
        if !result.scale.iter().chain(result.translation.iter()).all(|v|v.is_finite()){return Err("gesture overflow".into());}Ok(result)
    }
    pub fn glow_target(&self)->Result<Glow>{
        let p=self.profile()?.values;
        if self.reduce_highlighting{return Ok(Glow{opacity:0.,big_opacity:0.,scale:1.,diameter:0.,position:[0.,0.]});}
        let [x,y]=self.translation;let far=(x*x+y*y).sqrt()>p[3];
        let position=if self.active{[self.start[0]+x,self.start[1]+y]}else{[0.,0.]};
        if !position.iter().all(|v|v.is_finite()){return Err("gesture overflow".into());}
        Ok(Glow{opacity:if self.active{p[2]*if far{0.5}else{1.}}else{0.},big_opacity:if self.active{p[1]}else{0.},scale:if self.active{if far{2.}else{1.}}else{4.},diameter:(1.5*self.size[0].min(self.size[1])).min(160.),position})
    }
}

/// Scalar spring state driven by caller timestamps in seconds.
/// The integration and completion checks follow the 26A434 interaction oracle.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    offset: f64,
    velocity: f64,
    force: f64,
    time: f64,
    last_request: f64,
}

impl Spring {
    pub fn new(offset: f64, time: f64) -> Result<Self> {
        if !offset.is_finite() || !valid_time(time) {
            return Err("finite spring offset and timestamp required".into());
        }
        Ok(Self { offset, velocity: 0.0, force: 0.0, time, last_request: time })
    }

    /// Return the current offset, or None when the caller can present the target.
    pub fn advance(
        &mut self,
        target: f64,
        time: f64,
        response: f64,
        damping: f64,
    ) -> Result<Option<f64>> {
        if !valid_time(time) || time < self.last_request || !target.is_finite()
            || !response.is_finite() || response <= 0.0 || !damping.is_finite() || damping < 0.0
        {
            return Err("invalid spring parameters or nonmonotonic time".into());
        }
        const STEP: f64 = 1.0 / 300.0;
        let frequency = std::f64::consts::TAU / response;
        let stiffness = (frequency * frequency).min(44_999.999_999_999_99);
        let drag = -damping * (2.0 * stiffness.sqrt());
        let mut next = *self;
        next.last_request = time;
        if time - next.time > 1.0 {
            next.time = time - 1.0 / 60.0;
        }
        while next.time < time {
            next.velocity += next.force * (STEP * 0.5);
            next.offset += next.velocity * STEP;
            next.force = drag * next.velocity + stiffness * (target - next.offset);
            next.velocity += next.force * (STEP * 0.5);
            next.time += STEP;
        }
        if ![next.offset, next.force, next.velocity].iter().all(|x| x.is_finite()) {
            return Err("spring arithmetic overflow".into());
        }
        *self = next;
        let tolerance = target * 0.01;
        let difference = target - next.offset;
        let moving = (next.velocity * next.velocity).max(next.force * next.force) > 0.0036;
        let outside_tolerance = tolerance != 0.0 && tolerance * tolerance < difference * difference;
        Ok((moving || outside_tolerance).then_some(next.offset))
    }
}

fn valid_time(time: f64) -> bool {
    // Bound the elapsed time to precision where a 300 Hz step always advances it.
    time.is_finite() && time.abs() <= 1.0e12
}
