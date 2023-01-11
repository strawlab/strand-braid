pub fn init() {
    // This sends all log events using the `log` crate into the `tracing`
    // infrastructure.
    tracing_subscriber::fmt::init();
}
