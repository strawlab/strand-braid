#[derive(Debug, Copy, Clone)]
pub struct Vert {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

// This depends on a macro from the `glium` crate.
implement_vertex!(Vert, position, tex_coords);

pub static VERTEX_DATA: [Vert; 4] = [
    Vert {
        position: [-1.0, -1.0],
        tex_coords: [0.0, 0.0],
    },

    Vert {
        position: [-1.0, 1.0],
        tex_coords: [0.0, 1.0],
    },

    Vert {
        position: [1.0, 1.0],
        tex_coords: [1.0, 1.0],
    },

    Vert {
        position: [1.0, -1.0],
        tex_coords: [1.0, 0.0],
    },
];

#[rustfmt::skip]
pub const INDEX_DATA: [u16; 6] = [
        0, 1, 2,
        0, 2, 3,
];
