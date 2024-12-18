use eyre::Result;

lazy_static::lazy_static! {
    static ref PYLON_MODULE: ci2_pyloncxx::WrappedModule = ci2_pyloncxx::new_module().unwrap();
}

fn main() -> Result<()> {
    let guard = ci2_pyloncxx::make_singleton_guard(&&*PYLON_MODULE)?;
    let mymod = ci2_async::into_threaded_async(&*PYLON_MODULE, &guard);
    strand_cam::cli_app::cli_main(mymod, env!("CARGO_PKG_NAME"))?;
    Ok(())
}
