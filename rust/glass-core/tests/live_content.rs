//! Compare every increment with a fresh render of the same content and fixed scene.
use liquid_glass_core::{
    Renderer,
    raster::{Content, Patch, PixelRect, Scale, Viewport},
    scene::{self, Backdrop, Group, LiveScene, Material, MaterialKind, Shape},
};

fn words(buf: &mut [u8], offset: usize, values: &[f32]) {
    for (chunk, value) in buf[offset..offset + values.len()*4].chunks_exact_mut(4).zip(values) {
        chunk.copy_from_slice(&value.to_ne_bytes());
    }
}

fn background_uniforms(viewport: Viewport) -> ([u8; 320], [u8; 32]) {
    let mut uniforms = [0; 320];
    let mut extension = [0; 32];
    let factor = viewport.scale().factor() as f32;
    let (w,h) = (viewport.points().width as f32, viewport.points().height as f32);
    for (offset, values) in [
        (0, &[1.0/w,0.0,0.0,1.0/h][..]),
        (16, &[7.0,0.2,3.0,0.5,-12.0,-1.0]),
        (40, &[3.5*factor,1.25*factor,2.75,0.6,3.0,0.5]),
        (64, &[-2.5/w,1.5/h,2.0*factor,0.25]),
        (152, &[0.5,0.7,1.0,0.0,0.0,0.0]),
        (176, &[-30.0,-12.0,-3.0,-0.5,-2.0,-1.0,0.45,0.5,-1.0,1.0]),
        (216, &[0.0,0.7,0.3]),
        (240, &[0.0,-1.0/h,1.0,4.0,0.3,0.7]),
        (264, &[0.5,0.86,1.5,0.35,0.7,-0.2,0.1,1.5*factor,0.1,0.24,0.18,0.7,0.55,0.83]),
    ] {
        words(&mut uniforms,offset,values);
    }
    for offset in [80,104,128] {
        for i in 0..3 {
            let at = offset+i*10;
            uniforms[at..at+2].copy_from_slice(&0x3c00u16.to_ne_bytes());
        }
    }
    words(&mut extension,0,&[0.7,0.0,1.0,1.0]);
    extension[21]=1;
    extension[23]=1;
    (uniforms,extension)
}

fn contains(rect: PixelRect, x: u32, y: u32) -> bool {
    (rect.x..rect.x+rect.width).contains(&x) && (rect.y..rect.y+rect.height).contains(&y)
}

#[test]
fn background_patches_compose_all_materials_and_feed_the_next_group() {
    for scale in [Scale::One,Scale::Two] {
        let viewport = Viewport::new(197,157,scale).unwrap();
        let size = viewport.pixels();
        let factor = scale.factor();
        let (w,h) = (viewport.points().width as f32,viewport.points().height as f32);
        let (uniforms,extension) = background_uniforms(viewport);
        let mut foreground = [0;80];
        words(&mut foreground,0,&[1.0/w,0.0,0.0,1.0/h,7.0,0.2,0.0,4.0,0.12,0.0,
            0.65,0.76,-1.0,1.0,0.2,0.4,0.45,0.89]);
        let shapes = [58.137,109.273,146.193].map(|cx| Shape::Geometry {
            values: [[37.0,34.0,4.0,0.0],[1.0,0.0,0.0,1.0],[1.0,1.0,15.0,0.0]],
            transform: [[w,0.0,-cx],[0.0,h,-78.193]],
        });
        let mut original = Vec::with_capacity(size.pixels()*4);
        for y in 0..size.height {
            for x in 0..size.width {
                original.extend([0.24+(x%17) as f32/77.0,0.39+(y%11) as f32/66.0,0.38,1.0]);
            }
        }
        let mut renderer = Renderer::new(size).unwrap();
        let variants = [MaterialKind::Foreground,MaterialKind::Minimal,MaterialKind::Edge,
            MaterialKind::Refraction,MaterialKind::RefractionEdge,MaterialKind::Composite,
            MaterialKind::CompositeEdge,MaterialKind::CompositeRefraction,MaterialKind::All];
        for kind in variants {
            let groups = [&shapes[..2],&shapes[2..]].map(|shapes| Group {
                shapes, opacity: 0.75, smoothness: 6.0,
                blur_levels: if scale==Scale::One { 2 } else { 3 },
                material: if matches!(kind,MaterialKind::Foreground) {
                    Material {kind,uniforms:&foreground,extension:&[]}
                } else {
                    Material {kind,uniforms:&uniforms,extension:&extension}
                },
            });
            for backdrop in [Backdrop::Original,Backdrop::Current] {
                let mut source = Content::new(viewport,1,original.clone()).unwrap();
                let mut live = LiveScene::new(&mut renderer,source.clone(),backdrop,&groups,32*1024*1024).unwrap();
                let mut previous_rect: Option<PixelRect> = None;
                for (step,(x,y)) in [(85,68),(110,89),(2,150)].into_iter().enumerate() {
                    let rect = PixelRect {x:x*factor,y:y*factor,width:5*factor,height:5*factor};
                    let replacement = [0.98,0.06,0.12,1.0].repeat((rect.width*rect.height) as usize);
                    let restore;
                    let mut patches = Vec::new();
                    if let Some(previous) = previous_rect {
                        restore = (previous.y..previous.y + previous.height).flat_map(|row| {
                            let start = ((row * size.width + previous.x) * 4) as usize;
                            original[start..start + (previous.width * 4) as usize].iter().copied()
                        }).collect::<Vec<_>>();
                        patches.push(Patch {rect:previous,rgba:&restore});
                    }
                    patches.push(Patch {rect,rgba:&replacement});
                    let before = live.pixels().to_vec();
                    let version = step as u64+2;
                    let dirty = live.update(&mut renderer,version,&patches).unwrap();
                    source.update(version,&patches).unwrap();
                    let full = scene::render(&mut renderer,&source,backdrop,&groups).unwrap();
                    assert_eq!(live.version(),version);
                    let mut carried_beyond_input = false;
                    for (i,((actual,expected),old)) in live.pixels().chunks_exact(4).zip(full.chunks_exact(4))
                        .zip(before.chunks_exact(4)).enumerate() {
                        let x = i as u32%size.width;
                        let y = i as u32/size.width;
                        assert_eq!(actual,expected,"scale {factor}, variant {}, version {version}, pixel {x},{y}",kind as u8);
                        if actual!=old {
                            assert!(contains(dirty,x,y));
                            carried_beyond_input |= !contains(rect,x,y);
                        }
                    }
                    if step==0 {
                        assert!(carried_beyond_input);
                    }
                    // Leave some pixels untouched, even with two accumulated groups.
                    assert!((dirty.width*dirty.height) < size.width*size.height);
                    previous_rect = Some(rect);
                }
            }
        }
    }
}

#[test]
fn a_failed_draw_or_bad_batch_keeps_the_previous_version_and_pixels() {
    let viewport = Viewport::new(15,9,Scale::One).unwrap();
    let size = viewport.pixels();
    let content = Content::new(viewport,7,[0.2,0.2,0.2,1.0].repeat(size.pixels())).unwrap();
    let groups = [];
    let mut renderer = Renderer::new(size).unwrap();
    let mut scene = LiveScene::new(&mut renderer,content,Backdrop::Current,&groups,4096).unwrap();
    let rect = PixelRect {x:5,y:4,width:1,height:1};
    let patch = Patch {rect,rgba:&[0.9,0.3,0.2,1.0]};
    let before = scene.pixels().to_vec();
    assert!(scene.update(&mut renderer,7,&[Patch {..patch}]).is_err());
    let invalid = Patch {rect:PixelRect {x:15,..rect},rgba:patch.rgba};
    assert!(scene.update(&mut renderer,8,&[Patch {..patch},invalid]).is_err());
    let mut wrong_target = Renderer::new(Viewport::new(15,9,Scale::Two).unwrap().pixels()).unwrap();
    assert!(scene.update(&mut wrong_target,8,&[Patch {..patch}]).is_err());
    assert_eq!(scene.version(),7);
    assert_eq!(scene.pixels(),before);
    assert_eq!(scene.update(&mut renderer,8,&[patch]).unwrap(),rect);
    assert_eq!(scene.version(),8);

    let (uniforms,extension) = background_uniforms(viewport);
    let shapes = [Shape::Geometry {
        values: [[4.0,3.0,4.0,0.0],[1.0,0.0,0.0,1.0],[1.0,1.0,2.0,0.0]],
        transform: [[15.0,0.0,-7.13],[0.0,9.0,-4.23]],
    }];
    let groups = [Group {
        shapes:&shapes, opacity:1.0, smoothness:0.0, blur_levels:1,
        material:Material {kind:MaterialKind::Minimal,uniforms:&uniforms,extension:&extension},
    }];
    let content = Content::new(viewport,8,[0.2,0.2,0.2,1.0].repeat(size.pixels())).unwrap();
    let patch = Patch {rect,rgba:&[0.9,0.3,0.2,1.0]};
    assert!(LiveScene::new(&mut renderer,content.clone(),Backdrop::Current,&groups,4096).is_err());
    let mut scene = LiveScene::new(&mut renderer,content,Backdrop::Current,&groups,8192).unwrap();
    let mut limited = Renderer::with_budget(size,size.pixels()*16).unwrap();
    let before = scene.pixels().to_vec();
    assert!(scene.update(&mut limited,9,&[Patch {..patch}]).is_err());
    assert_eq!(scene.version(),8);
    assert_eq!(scene.pixels(),before);
    scene.update(&mut renderer,9,&[patch]).unwrap();
    assert_eq!(scene.version(),9);
    assert_ne!(scene.pixels(),before);
}
