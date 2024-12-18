use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use machine_vision_formats::{pixel_format::Mono8, ImageData};

use std::sync::{
    mpsc::{Receiver, Sender},
    Arc,
};

pub(crate) type ImType = basic_frame::BasicFrame<Mono8>;

pub struct StrandCamEguiApp {
    cmd_tx: tokio::sync::mpsc::Sender<()>,
    gui_singleton: crate::ArcMutGuiSingleton,
    version_string: String,
    frame_rx: Receiver<ImType>,
    egui_ctx_tx: Option<Sender<egui::Context>>,
    screen_texture: Option<TextureHandle>,
}

impl StrandCamEguiApp {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<()>,
        cc: &eframe::CreationContext<'_>,
        gui_singleton: crate::ArcMutGuiSingleton,
        frame_rx: Receiver<ImType>,
        egui_ctx_tx: Sender<egui::Context>,
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
            frame_rx,
            egui_ctx_tx: Some(egui_ctx_tx),
            screen_texture: None,
        }
    }
}

impl eframe::App for StrandCamEguiApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Ignore only possible error of SendError which we could get if the
        // receiver hung up.
        let _ = self.cmd_tx.blocking_send(());
    }

    fn update(&mut self, ctx: &egui::Context, _egui_frame: &mut eframe::Frame) {
        let Self {
            cmd_tx,
            gui_singleton,
            version_string,
            frame_rx,
            egui_ctx_tx,
            screen_texture,
        } = self;

        // If we still have the context sender, we need to clone the context to
        // send it.
        let do_ctx_clone = egui_ctx_tx.is_some();

        // Copy stuff from behind mutex.
        let (url_string, opt_ctx_clone) = {
            // scope for guard
            let my_guard = gui_singleton.lock().unwrap();

            // Copy the egui context if needed.
            let opt_ctx_clone = if do_ctx_clone {
                Some(my_guard.ctx.as_ref().unwrap().clone())
            } else {
                None
            };

            // Copy the URL if present.
            match my_guard.url.as_ref() {
                Some(url) => (Some(format!("{url}")), opt_ctx_clone),
                None => (None, opt_ctx_clone),
            }
        };

        // Send the egui context.
        if let Some(sender) = egui_ctx_tx.take() {
            if let Some(ctx_clone) = opt_ctx_clone {
                // On first update, send a clone of the egui context.
                sender.send(ctx_clone).unwrap();
            } else {
                unreachable!();
            }
        }

        let mut do_exit = false;
        match frame_rx.try_recv() {
            Ok(wrapped) => {
                let w = wrapped.width();
                let h = wrapped.height();
                let screen_texture = screen_texture.get_or_insert_with(|| {
                    ctx.load_texture(
                        "screen",
                        egui::ImageData::Color(Arc::new(ColorImage::new(
                            [w as usize, h as usize],
                            Color32::TRANSPARENT,
                        ))),
                        TextureOptions::default(),
                    )
                });

                let buf: Vec<u8> = wrapped.into();
                screen_texture.set(
                    ColorImage::from_gray([w as usize, h as usize], &buf),
                    TextureOptions::default(),
                );
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // sender hung up
                tracing::error!("Camera thread disconnected");
                do_exit = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                egui::warn_if_debug_build(ui);
                ui.heading("Strand Camera");

                if do_exit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                if let Some(tex) = self.screen_texture.as_ref() {
                    ui.add(egui::Image::new(tex).shrink_to_fit());
                }

                {
                    if ui.button("Quit").clicked() {
                        // Ignore only possible error of SendError which we could
                        // get if the receiver hung up.
                        let _ = cmd_tx.blocking_send(());
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        // frame.close();
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
            });
        });
    }
}
