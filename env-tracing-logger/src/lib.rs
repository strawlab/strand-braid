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
    let file_layer = if let Some(path) = &path {
        let file = std::fs::File::create(path)?;
        let file_writer = std::sync::Mutex::new(file);
        Some(
            fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true),
        )
    } else {
        None
    };

    let console_layer = if disable_console {
        None
    } else {
        let with_ansi = !cfg!(windows) ;
        Some(fmt::layer().with_ansi(with_ansi).with_file(true).with_line_number(true))
    };

    let collector = tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .with(tracing_subscriber::filter::EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(collector)?;

    let log_var = if let Ok(var) = std::env::var("RUST_LOG") {
        format!(" with RUST_LOG=\"{}\".", var)
    } else {
        ".".to_string()
    };

    if let Some(path) = &path {
        tracing::debug!(
            "Logging initiated to file \"{}\"{log_var}",
            path.as_ref().display(),
        );
    }

    if !disable_console {
        tracing::debug!("Logging initiated to console{log_var}",);
    }

    Ok(Guard {})
}
