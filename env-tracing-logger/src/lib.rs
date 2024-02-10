use tracing_subscriber::{fmt, layer::SubscriberExt};

struct Guard {}

impl Drop for Guard {
    fn drop(&mut self) {}
}

pub fn init() -> impl Drop {
    initiate_logging::<&str>(None, false).unwrap()
}

/// Start logging to file and console, both optional.
pub fn initiate_logging<P: AsRef<std::path::Path>>(
    path: Option<P>,
    disable_console: bool,
) -> Result<impl Drop, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let file_layer = if let Some(path) = path {
        let file = std::fs::File::create(path)?;
        let file_writer = std::sync::Mutex::new(file);
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
    } else {
        None
    };

    let console_layer = if disable_console {
        None
    } else {
        Some(fmt::layer().with_timer(tracing_subscriber::fmt::time::Uptime::default()))
    };

    let collector = tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .with(tracing_subscriber::filter::EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(collector)?;
    Ok(Guard {})
}
