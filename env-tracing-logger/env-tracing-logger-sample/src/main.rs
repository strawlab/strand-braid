#[tracing::instrument]
fn my_span(arg1: u8) {
    tracing::info!("From inside span: {}.", arg1);
}

fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    let _tracing_guard = env_tracing_logger::init();
    tracing::info!("Hello, world!");
    log::info!("This is from log (not tracing).");
    tracing::trace!("This is at the trace level...");
    my_span(42);
}
