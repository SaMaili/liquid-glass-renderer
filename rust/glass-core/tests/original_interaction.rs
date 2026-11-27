use liquid_glass_core::interaction::*;
#[test]
fn executed_original_targets(){
    let fixture=include_str!("../../../work/size-profiles/rust-oracle.txt");let mut cases=0;
    for line in fixture.lines(){
        let row:Vec<f64>=line.split_whitespace().map(|s|s.parse().unwrap()).collect();
        let g=Gesture{size:[row[0],row[1]],input:if row[2]==1.{Input::Touch}else{Input::Cursor},start:[row[3],row[4]],translation:[row[5],row[6]],active:row[7]==1.,reduce_highlighting:row[8]==1.,lift:row[9]==1.,flex:row[10]==1.};
        assert_eq!(Profile::automatic(g.size,g.input).unwrap().values.as_slice(),&row[11..29]);
        let flex=g.flex_target().unwrap();let actual=[flex.scale,flex.translation,flex.spring].concat();assert_eq!(actual,&row[29..35]);
        let glow=g.glow_target().unwrap();assert_eq!([glow.opacity,glow.big_opacity,glow.scale,glow.diameter,glow.position[0],glow.position[1]],row[35..41]);cases+=1;
    }
    assert_eq!(cases,2192);
}

#[test]
fn executed_original_spring_frames() {
    let fixture = include_str!("../../../work/size-profiles/rust-spring-oracle.txt");
    let mut spring = Spring::new(0.0, 0.0).unwrap();
    let mut maximum_error = 0.0f64;
    let mut count = 0;
    for line in fixture.lines() {
        let row: Vec<f64> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
        let [frame, initial, time, target, response, damping, value, complete] = row[..] else {
            panic!("unexpected spring fixture row");
        };
        if frame == 0.0 {
            spring = Spring::new(initial, time).unwrap();
        }
        let result = spring.advance(target, time, response, damping).unwrap();
        assert_eq!(result.is_none(), complete == 1.0, "row {count}, frame {frame}");
        maximum_error = maximum_error.max((result.unwrap_or(target) - value).abs());
        count += 1;
    }
    assert_eq!(count, 968);
    println!("{count} original spring frames, maximum error {maximum_error}");
    assert_eq!(maximum_error, 0.0);
}
