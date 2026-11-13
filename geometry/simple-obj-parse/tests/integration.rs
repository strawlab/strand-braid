// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[test]
fn test_read_32vert_polygon() {
    // this causes a panic in `obj` crate 0.9. See
    // https://github.com/kvark/obj/pull/10
    let buf = include_bytes!("has-32vert-polygon.obj");

    // This might fail but it should not panic. See
    // https://github.com/kvark/obj/pull/10
    let _ = simple_obj_parse::obj_parse(buf).is_ok();
}

#[test]
fn test_read_tetrahedron() {
    let buf = include_bytes!("tetrahedron.obj");
    simple_obj_parse::obj_parse(buf).expect("parsed");
}

/// A unit square made of one triangle and one quad, with texture coordinates.
const TRI_AND_QUAD: &[u8] = b"o shape
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 2 0 0
vt 0 0
vt 1 0
vt 1 1
vt 0 1
vt 0.5 0.5
f 1/1 2/2 3/3 4/4
f 2/2 5/5 3/3
";

#[test]
fn test_quad_split_into_two_triangles() {
    let meshes = simple_obj_parse::obj_parse(TRI_AND_QUAD).expect("parsed");
    assert_eq!(meshes.len(), 1);
    let (name, mesh) = &meshes[0];
    assert_eq!(name, "shape");
    // The quad 1 2 3 4 becomes triangles 1 2 3 and 3 4 1, then the triangle
    // 2 5 3. Each triangle gets its own copy of its vertices.
    let p = |x, y| [x, y, 0.0];
    assert_eq!(
        mesh.coords,
        [
            p(0.0, 0.0),
            p(1.0, 0.0),
            p(1.0, 1.0),
            p(1.0, 1.0),
            p(0.0, 1.0),
            p(0.0, 0.0),
            p(1.0, 0.0),
            p(2.0, 0.0),
            p(1.0, 1.0),
        ]
    );
    assert_eq!(mesh.uvs[3..6], [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]);
    assert_eq!(mesh.indices, [[0, 1, 2], [3, 4, 5], [6, 7, 8]]);
}

#[test]
fn test_pentagon_is_an_error() {
    let buf = b"v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 2 0
vt 0 0
f 1/1 2/1 3/1 4/1 5/1
";
    let err = simple_obj_parse::obj_parse(buf).unwrap_err();
    assert!(err.to_string().contains('5'), "{err}");
}
