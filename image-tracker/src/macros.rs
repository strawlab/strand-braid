#[cfg(feature = "debug-images")]
macro_rules! image_debug {
    ($im:expr, $name:expr) => {{
        RT_IMAGE_VIEWER_SENDER.with(|sender| {
            (*sender)
                .borrow_mut()
                .send($im, $name)
                .expect("rt image viewer sender");
        });
    }};
}

#[cfg(not(feature = "debug-images"))]
macro_rules! image_debug {
    ($im:expr, $name:expr) => {{}};
}
