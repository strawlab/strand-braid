use tracing_subscriber::prelude::*;

// struct Guard {}

// impl Drop for Guard {
//     fn drop(&mut self) {}
// }

pub fn init() -> impl Drop {
    init_result()
        .map_err(|e| e.1)
        .expect("Could not set global default")
}

fn init_result() -> Result<impl Drop, (impl Drop, tracing::subscriber::SetGlobalDefaultError)> {
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

    match tracing::subscriber::set_global_default(subscriber) {
        Ok(_) => Ok(_guard),
        Err(e) => Err((_guard, e)),
    }
}
