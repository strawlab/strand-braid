use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub fn init() {
    // This sends all log events using the `log` crate into the `tracing`
    // infrastructure. The documentation
    // [here](https://docs.rs/crate/tracing/0.1.22/source/README.md) says "Note
    // that if you're using tracing-subscriber's FmtSubscriber, you don't need
    // to depend on tracing-log directly". However, I did not find this to be
    // true.
    tracing_log::env_logger::init();

    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
