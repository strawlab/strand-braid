use std::path::Path;
use std::fs::File;
use std::io::Read;
use rustc_serialize::json;

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub struct Config {
    pub vr_display: VRDisplayConfig,
    pub image_processing: ImageProcessingConfig,
    pub tracker: TrackerConfig,
    pub network_listener: NetworkListenerConfig,
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub enum DarkOrLight {
    Dark,
    Light,
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub enum CameraOrNetwork {
    Camera,
    Network,
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub struct ImageProcessingConfig {
    pub detect_dark_or_light: DarkOrLight,
    pub show_camera_window: bool,
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub struct NetworkListenerConfig {
    pub socket_addr: String,
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub struct TrackerConfig {
    pub use_camera_or_network: CameraOrNetwork,
    pub pixels_to_meters_matrix3: [[f32; 3]; 3],
}

#[derive(Clone, Debug, RustcDecodable, RustcEncodable)]
pub struct VRDisplayConfig {
    pub show_vr_window: bool,
    pub model_fname: String,
    pub vert_shader_fname: String,
    pub frag_shader_fname: String,
    pub width_meters: f32,
    pub height_meters: f32,
    pub window_preferred_width_pixels: u32,
    pub window_preferred_height_pixels: u32,
    pub show_overview: bool,
    pub overview_cam_x: f32,
    pub overview_cam_y: f32,
    pub overview_cam_z: f32,
    pub screen_above_observer: bool,
    pub distance_to_screen_meters: f32,
    pub far_clip_meters: f32,
}

impl Config {
    pub fn new_from_file(path: &Path) -> Config {
        let mut file = File::open(&path).expect("opening config file");

        let mut buf = String::new();
        file.read_to_string(&mut buf).expect("reading config file");

        match json::decode(&buf) {
            Ok(decoded) => decoded,
            Err(e) => {
                panic!("failed to decode JSON config file {:?}: {}", path, e);
            }
        }
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            vr_display: VRDisplayConfig {
                show_vr_window: true,
                model_fname: String::from("capsule.obj"),
                vert_shader_fname: String::from("vertex_shader.glsl"),
                frag_shader_fname: String::from("fragment_shader.glsl"),
                width_meters: 0.32,
                height_meters: 0.18,
                window_preferred_width_pixels: 600,
                window_preferred_height_pixels: 400,
                show_overview: true,
                overview_cam_x: -0.3,
                overview_cam_y: -0.5,
                overview_cam_z: 0.6,
                screen_above_observer: true,
                distance_to_screen_meters: 0.004,
                far_clip_meters: 10.0,
            },
            image_processing: ImageProcessingConfig {
                detect_dark_or_light: DarkOrLight::Light,
                show_camera_window: true,
            },
            tracker: TrackerConfig {
                use_camera_or_network: CameraOrNetwork::Network,
                pixels_to_meters_matrix3: [[0.001, 0.0, 0.0], [0.0, 0.001, 0.0], [0.0, 0.0, 1.0]],
            },
            network_listener: NetworkListenerConfig { socket_addr: "0.0.0.0:3443".to_string() },
        }
    }
}
