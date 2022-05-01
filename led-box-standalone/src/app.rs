use std::{str, sync::Arc};

use parking_lot::Mutex;

use eframe::{egui, epi};

use log::info;

use crate::box_status::{BoxManager, BoxStatus, Cmd};

pub struct TemplateApp {
    available_ports: Vec<String>,

    box_manager: Arc<Mutex<BoxManager>>,
    cmd_tx: tokio::sync::mpsc::Sender<Cmd>,
}

impl TemplateApp {
    pub fn new(
        available_ports: Vec<String>,
        box_manager: Arc<Mutex<BoxManager>>,
        cmd_tx: tokio::sync::mpsc::Sender<Cmd>,
    ) -> Self {
        Self {
            available_ports,
            box_manager,
            cmd_tx,
        }
    }
}

impl epi::App for TemplateApp {
    fn name(&self) -> &str {
        "LED box control"
    }

    fn on_exit(&mut self) {
        info!("app sending Cmd::Quit command to serial loop");
        self.cmd_tx.blocking_send(Cmd::Quit).unwrap();
    }

    /// Called once before the first frame.
    fn setup(
        &mut self,
        _ctx: &egui::Context,
        _frame: &epi::Frame,
        _storage: Option<&dyn epi::Storage>,
    ) {
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, _frame: &epi::Frame) {
        let Self {
            available_ports,
            box_manager,
            cmd_tx,
        } = self;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("LED box control");

            {
                let status = box_manager.lock().status();
                match &status {
                    BoxStatus::Unconnected => {
                        ui.label(" Unconnected ");
                        if available_ports.is_empty() {
                            ui.label(" No connected devices.");
                        } else {
                            for port in available_ports.iter() {
                                if ui.button(port).clicked() {
                                    cmd_tx.blocking_send(Cmd::Connect(port.clone())).unwrap();
                                }
                            }
                        }
                    }
                    &BoxStatus::Connected(_state) => {
                        ui.label(" Connected ");
                        for chan in &[1, 2, 3] {
                            let label = format!("Toggle LED {}", chan);
                            if ui.button(label).clicked() {
                                cmd_tx.blocking_send(Cmd::Toggle(*chan)).unwrap();
                            }
                        }
                    }
                }
            }
            egui::warn_if_debug_build(ui);
        });
    }
}
