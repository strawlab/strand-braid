#[test]
fn test_read_32vert_polygon() {
    // this causes a panic in `obj` crate 0.9. See
    // https://github.com/kvark/obj/pull/10
    let buf = include_bytes!("has-32vert-polygon.obj");

    // This might fail but it should not panic. See
    // https://github.com/kvark/obj/pull/10
    match simple_obj_parse::obj_parse(buf) {
        Ok(_) => {},
        Err(_) => {},
    };
}

#[test]
fn test_read_tetrahedron() {
    let buf = include_bytes!("tetrahedron.obj");
    simple_obj_parse::obj_parse(buf).expect("parsed");
}
