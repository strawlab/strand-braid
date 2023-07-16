use tracing::subscriber::SetGlobalDefaultError;
use tracing_subscriber::{
    fmt::{self, time, format},
    prelude::*,
    EnvFilter,
};

struct Guard {}

impl Drop for Guard {
    fn drop(&mut self) {}
}

pub fn init() -> impl Drop {
    init_result()
        .map_err(|e| e.1)
        .expect("Could not set global default")
}

fn init_result() -> Result<impl Drop, (impl Drop, tracing::subscriber::SetGlobalDefaultError)> {
    let fmt = format().with_timer(time::Uptime::default());
    let fmt_layer = fmt::layer().event_format(fmt).with_target(false);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(EnvFilter::from_default_env())
        .init();

    let _guard = Guard {};

    Ok::<_, (Guard, SetGlobalDefaultError)>(_guard)
}
