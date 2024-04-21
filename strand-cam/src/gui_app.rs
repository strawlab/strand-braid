use eframe::egui;

pub struct StrandCamEguiApp {
    cmd_tx: tokio::sync::mpsc::Sender<()>,
    gui_singleton: crate::ArcMutGuiSingleton,
    version_string: String,
}

impl StrandCamEguiApp {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<()>,
        cc: &eframe::CreationContext<'_>,
        gui_singleton: crate::ArcMutGuiSingleton,
    ) -> Self {
        {
            // update gui singleton with the egui context.
            let mut my_guard = gui_singleton.lock().unwrap();
            my_guard.ctx = Some(cc.egui_ctx.clone());
        }

        let version_string = format!("Strand Camera version: {}", env!("CARGO_PKG_VERSION"));
        Self {
            cmd_tx,
            gui_singleton,
            version_string,
        }
    }
}

impl eframe::App for StrandCamEguiApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Ignore only possible error of SendError which we could get if the
        // receiver hung up.
        let _ = self.cmd_tx.blocking_send(());
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            cmd_tx,
            gui_singleton,
            version_string,
        } = self;

        let url_string = {
            match gui_singleton.lock().unwrap().url.as_ref() {
                Some(url) => Some(format!("{url}")),
                None => None,
            }
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Strand Camera");

            {
                if ui.button("Quit").clicked() {
                    // Ignore only possible error of SendError which we could
                    // get if the receiver hung up.
                    let _ = cmd_tx.blocking_send(());
                    frame.close();
                }

                match url_string {
                    Some(mut url) => {
                        ui.label("URL");
                        ui.text_edit_singleline(&mut url);
                    }
                    None => {
                        ui.label("waiting for GUI");
                    }
                }

                ui.label(version_string.as_str());
            }
            egui::warn_if_debug_build(ui);
        });
    }
}
