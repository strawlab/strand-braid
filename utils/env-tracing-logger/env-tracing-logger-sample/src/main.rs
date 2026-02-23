#[tracing::instrument]
fn my_span(arg1: u8) {
    tracing::info!("From inside span: {}.", arg1);
}

fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }

    let _tracing_guard = env_tracing_logger::init();
    tracing::info!("Hello, world!");
    log::info!("This is from log (not tracing).");
    tracing::trace!("This is at the trace level...");

    // tracing::if_log_enabled!(tracing::Level::TRACE, {
    //     println!("trace logging enabled at runtime")
    // });
    if tracing::Level::TRACE <= tracing::level_filters::STATIC_MAX_LEVEL {
        println!("you compiled with trace messages enabled");
    }
    my_span(42);
}
