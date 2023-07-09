use tracing_subscriber::prelude::*;

pub fn init() -> impl Drop {
    let fmt_layer = tracing_subscriber::fmt::Layer::default();

    let (flame_layer, _guard) = tracing_flame::FlameLayer::with_file("./tracing.folded").unwrap();

    let subscriber = tracing_subscriber::registry::Registry::default()
        .with(fmt_layer)
        .with(flame_layer);

    tracing_log::LogTracer::init().unwrap();

    tracing::subscriber::set_global_default(subscriber).expect("Could not set global default");
    _guard
}
