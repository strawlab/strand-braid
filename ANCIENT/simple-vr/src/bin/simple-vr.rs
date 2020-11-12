#[macro_use]
extern crate log;
extern crate env_logger;
extern crate eventual;
extern crate time;
extern crate rustc_serialize;
extern crate ipp;
extern crate machine_vision_formats;
extern crate cgmath;

#[macro_use]
extern crate glium;

#[macro_use]
extern crate lazy_static;

extern crate reactive_cam;
#[cfg(feature = "camiface")]
extern crate reactive_camiface;

#[cfg(feature = "flycap")]
extern crate reactive_cam_flycap;
extern crate clap;

use std::sync::{Arc, Mutex};
use std::path::Path;
use std::net::ToSocketAddrs;

use glium::{glutin, Surface};
use clap::Arg;

extern crate simple_vr;

use simple_vr::{vr_display, config, camera, tracker, network_input};

fn empty_loop() {

    let mut events_loop = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new().with_title("no camera found");
    let context = glutin::ContextBuilder::new().with_vsync(false);
    let display = glium::Display::new(window, context, &events_loop).unwrap();
        // .with_dimensions(640, 480)

    let mut running = true;
    while running {
        let mut target = display.draw();
        target.clear_color(0.0, 0.0, 1.0, 1.0);
        target.finish().unwrap();
        events_loop.poll_events(|ev| match ev {
            glutin::Event::WindowEvent { event, .. } => {
                match event {
                    glutin::WindowEvent::Closed => running = false,
                    glutin::WindowEvent::KeyboardInput {
                        input, ..
                    } if glutin::ElementState::Pressed == input.state => {
                        if let glutin::VirtualKeyCode::Escape = key {
                            running = false
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        });
    }
}

fn run_camera(tracker: Arc<Mutex<tracker::Tracker>>, cfg: &config::ImageProcessingConfig) {
    let show_camera = cfg.show_camera_window;
    let cam_app_arc = Arc::new(Mutex::new(camera::CameraHolderApp::new(tracker, show_camera, cfg)));
    let mut sources = vec![];
    camera::maybe_insert_cam_iface(&mut sources, cam_app_arc.clone());
    camera::maybe_insert_flycap(&mut sources, cam_app_arc.clone());
    let mut cam_app = cam_app_arc.lock().unwrap();
    if cam_app.get_n_cams() == 0 {
        error!("no cameras detected");
        if show_camera {
            empty_loop();
        }
        return;
    }
    let mut running = true;
    while running {
        running = cam_app.camera_step();
    }
}

fn run_network(tracker: Arc<Mutex<tracker::Tracker>>, cfg: &config::NetworkListenerConfig) {
    let mut net_app = network_input::NetworkApp::new(tracker, cfg);
    let mut running = true;
    while running {
        running = net_app.network_step();
    }
}

fn run_vr(tracker: Arc<Mutex<tracker::Tracker>>,
          vr_cfg: &config::VRDisplayConfig,
          base_path: &Path) {
    let mut vr_display = vr_display::VRDisplay::new(vr_cfg, tracker, base_path);
    let mut running = true;
    while running {
        running = vr_display.display_step();
    }
}

fn main() {
    env_logger::init().unwrap();

    let matches = clap::App::new("Simple VR")
        .version("0.1")
        .arg(Arg::with_name("CONFIG")
            .help("Sets the VR configuration .json file to use")
            .required(true)
            .index(1))
        .get_matches();
    let cfg_fname = matches.value_of("CONFIG").unwrap();
    let cfg_path = Path::new(cfg_fname);

    let cfg = config::Config::new_from_file(cfg_path);
    let base_dir = cfg_path.parent()
        .expect("cfg path parent")
        .to_str()
        .expect("cfg path parent str");

    let vr_cfg = &cfg.vr_display;
    if vr_cfg.show_overview {
        if vr_cfg.overview_cam_x.is_nan() | vr_cfg.overview_cam_y.is_nan() |
           vr_cfg.overview_cam_z.is_nan() {
            panic!("overview camera postion cannot be at nan");
        }
    }
    let tracker = Arc::new(Mutex::new(tracker::Tracker::new(&cfg.tracker)));

    match vr_cfg.show_vr_window {
        true => {
            match cfg.tracker.use_camera_or_network {
                config::CameraOrNetwork::Camera => {
                    let tcopy = tracker.clone();
                    let timage_processing_cfg = cfg.image_processing.clone();
                    std::thread::spawn(move || {
                        run_camera(tcopy, &timage_processing_cfg);
                    });
                }
                config::CameraOrNetwork::Network => {

                    // ensure that we can parse socket address before launching thread.
                    let addrs: Vec<_> =
                        cfg.network_listener.socket_addr.to_socket_addrs().unwrap().collect();
                    assert!(addrs.len() == 1);

                    let tcopy = tracker.clone();
                    let network_listener_cfg = cfg.network_listener.clone();
                    std::thread::spawn(move || {
                        run_network(tcopy, &network_listener_cfg);
                    });
                }
            }
            run_vr(tracker, &vr_cfg, Path::new(base_dir));
        }
        false => {
            match cfg.tracker.use_camera_or_network {
                config::CameraOrNetwork::Camera => run_camera(tracker, &cfg.image_processing),
                config::CameraOrNetwork::Network => run_network(tracker, &cfg.network_listener),
            }
        }
    }
}
