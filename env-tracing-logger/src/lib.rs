use tracing_subscriber::prelude::*;

// struct Guard {}

// impl Drop for Guard {
//     fn drop(&mut self) {}
// }

pub fn init() -> impl Drop {
    // let fmt_layer = tracing_subscriber::fmt::Layer::default();

    let (flame_layer, _guard) = tracing_flame::FlameLayer::with_file("./tracing.folded").unwrap();

    // let subscriber = tracing_subscriber::registry::Registry::default()
    //     .with(fmt_layer)
    //     .with(flame_layer);

    let subscriber = tracing_subscriber::registry()
        // .with(tracing_subscriber::fmt::layer())
        // .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(flame_layer);
    // .init();

    // tracing_log::LogTracer::init().unwrap();

    // let _guard = Guard {};

    tracing::subscriber::set_global_default(subscriber).expect("Could not set global default");
    _guard
}
