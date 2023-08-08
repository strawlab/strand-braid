use tracing::{event::Event, subscriber::SetGlobalDefaultError, Level, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    fmt::{
        self,
        format::Writer,
        time::{self, FormatTime},
        FmtContext, FormatEvent, FormatFields,
    },
    prelude::*,
    registry::LookupSpan,
    EnvFilter,
};

struct MyEventFormat {
    timer: time::Uptime,
}

// This is mostly from the tracing_subscriber Compact formatter. As there was no
// way to suppress printing the context with Compact, I reimplemented this here.
// ANSI codes have been removed for simplicity but would be nice to add back.
impl<S, N> FormatEvent<S, N> for MyEventFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let normalized_meta = event.normalized_metadata();

        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());

        if self.timer.format_time(&mut writer).is_err() {
            writer.write_str("<unknown time>")?;
        }

        let fmt_level = match meta.level() {
            &Level::ERROR => "ERROR",
            &Level::WARN => " WARN",
            &Level::INFO => " INFO",
            &Level::DEBUG => "DEBUG",
            &Level::TRACE => "TRACE",
        };
        write!(writer, " {} ", fmt_level)?;

        write!(writer, "{}: ", meta.target())?;

        ctx.format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

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
    // let evt_fmt = format().with_timer(time::Uptime::default()).compact();
    let evt_fmt = MyEventFormat {
        timer: time::Uptime::default(),
    };
    let fmt_layer = fmt::layer().event_format(evt_fmt);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(EnvFilter::from_default_env())
        .init();

    let _guard = Guard {};

    Ok::<_, (Guard, SetGlobalDefaultError)>(_guard)
}
