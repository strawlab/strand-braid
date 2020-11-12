#[macro_use]
extern crate log;
extern crate env_logger;
extern crate cgmath;
extern crate imagefmt;
extern crate clap;
use clap::Arg;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug)]
struct ZCylinderConfig {
    n_grating_cycles: u32,
    grating_texture_width: u32,
    grating_texture_height: u32,
    n_segments: u32,
    radius: f32,
    height: f32,
    z0: f32,
}

impl Default for ZCylinderConfig {
    fn default() -> ZCylinderConfig {
        ZCylinderConfig {
            n_grating_cycles: 10,
            grating_texture_width: 1024,
            grating_texture_height: 64,
            n_segments: 128,
            radius: 1.0,
            height: 1.0,
            z0: -0.5,
        }
    }
}

#[derive(Debug)]
struct Vertex {
    position: cgmath::Vector3<f32>,
    normal: cgmath::Vector3<f32>,
    texcoord: cgmath::Vector2<f32>,
}

#[derive(Debug)]
struct Triangle {
    v0: u32,
    v1: u32,
    v2: u32,
}

impl Triangle {
    fn new(v0: u32, v1: u32, v2: u32) -> Triangle {
        Triangle {
            v0: v0,
            v1: v1,
            v2: v2,
        }
    }
}

struct Model {
    vertices: Vec<Vertex>,
    faces: Vec<Triangle>,
}

fn make_model(cfg: &ZCylinderConfig) -> Model {
    let dtheta = 2.0 * std::f32::consts::PI / (cfg.n_segments as f32);
    let z1 = cfg.z0 + cfg.height;
    let mut verts = vec![];
    let mut tris = vec![];

    // We compute n+1 vertices so that last face doesn't need to wrap.
    for i0 in 0..(cfg.n_segments + 1) {
        // compute vertices
        let theta = i0 as f32 * dtheta;
        let frac = (i0 as f32) / (cfg.n_segments as f32);
        verts.push(Vertex {
            position: cgmath::Vector3::new(f32::cos(theta), f32::sin(theta), cfg.z0),
            normal: cgmath::Vector3::new(f32::cos(theta), f32::sin(theta), 0.0),
            texcoord: cgmath::Vector2::new(frac, 0.0),
        });
        verts.push(Vertex {
            position: cgmath::Vector3::new(f32::cos(theta), f32::sin(theta), z1),
            normal: cgmath::Vector3::new(f32::cos(theta), f32::sin(theta), 0.0),
            texcoord: cgmath::Vector2::new(frac, 1.0),
        });

        // compute faces
        let i1 = i0 + 1;

        let v0 = i0 * 2;
        let v1 = i0 * 2 + 1;
        let v2 = i1 * 2;
        let v3 = i1 * 2 + 1;

        tris.push(Triangle::new(v0, v1, v2));
        tris.push(Triangle::new(v3, v2, v1));
    }

    // Remove faces for n+1 vertices.
    tris.pop();
    tris.pop();

    Model {
        vertices: verts,
        faces: tris,
    }
}

fn save_model(basename: &str, model: &Model) -> Result<String, std::io::Error> {
    let obj_name = basename.to_string() + ".obj";
    let mtl_name = basename.to_string() + ".mtl";
    let png_name = basename.to_string() + ".png";
    let f = try!(File::create(&obj_name));
    {
        let mut writer = BufWriter::new(f);

        let mtl_name_local = Path::new(&mtl_name).file_name().expect("mtl file name");
        try!(write!(&mut writer,
                    "mtllib {}\n",
                    mtl_name_local.to_str().expect("mtl name to string")));
        for v in model.vertices.iter() {
            let p = &v.position;
            try!(write!(&mut writer, "v {} {} {}\n", p.x, p.y, p.z));
        }

        for v in model.vertices.iter() {
            let p = &v.normal;
            try!(write!(&mut writer, "vn {} {} {}\n", p.x, p.y, p.z));
        }

        for v in model.vertices.iter() {
            let t = &v.texcoord;
            try!(write!(&mut writer, "vt {} {}\n", t.x, t.y));
        }

        try!(write!(&mut writer, "usemtl material0\n"));

        for f in model.faces.iter() {
            try!(write!(&mut writer,
                        "f {}/{}/{} {}/{}/{} {}/{}/{}\n",
                        f.v0 + 1,
                        f.v0 + 1,
                        f.v0 + 1,
                        f.v1 + 1,
                        f.v1 + 1,
                        f.v1 + 1,
                        f.v2 + 1,
                        f.v2 + 1,
                        f.v2 + 1));
        }

    }

    let f = try!(File::create(&mtl_name));
    {
        let mut writer = BufWriter::new(f);
        let png_name_local = Path::new(&png_name).file_name().expect("png file name");
        try!(write!(&mut writer,
                    "newmtl material0
Ka 1.000000 1.000000 1.000000
Kd 1.000000 1.000000 1.000000
Ks 0.000000 0.000000 0.000000
Tr 1.000000
illum 1
Ns 0.000000
map_Kd {}
",
                    png_name_local.to_str().expect("png name to string")));
    }
    Ok(png_name)
}

fn save_texture(png_name: &str, cfg: &ZCylinderConfig) -> Result<(), imagefmt::Error> {
    let image_row: Vec<u8> = (0..cfg.grating_texture_width)
        .map(|x| {
            let frac = x as f32 / cfg.grating_texture_width as f32;
            let theta = frac * 2.0 * std::f32::consts::PI;
            let val = f32::sin(theta * cfg.n_grating_cycles as f32);
            ((val + 1.0) * (255.0 / 2.0)) as u8
        })
        .collect::<Vec<u8>>();
    let mut imdata: Vec<u8> = Vec::with_capacity(cfg.grating_texture_height as usize *
                                                 image_row.len());
    for _ in 0..cfg.grating_texture_height {
        for x in image_row.iter() {
            imdata.push(*x);
        }
    }
    try!(imagefmt::write(png_name,
                         cfg.grating_texture_width as usize,
                         cfg.grating_texture_height as usize,
                         imagefmt::ColFmt::Y,
                         &imdata[..imdata.len()],
                         imagefmt::ColType::Auto));
    Ok(())
}

fn main() {
    env_logger::init().unwrap();

    let matches = clap::App::new("save-cylinder-model")
        .version("0.1")
        .arg(Arg::with_name("BASENAME")
            .help("Sets the output filename BASENAME.obj and BASENAME.mtl")
            .required(true)
            .index(1))
        .get_matches();
    let basename = matches.value_of("BASENAME").unwrap();
    let cfg = ZCylinderConfig::default();
    let model = make_model(&cfg);
    let texture_fname = save_model(basename, &model).unwrap();
    save_texture(&texture_fname, &cfg).unwrap();
}
