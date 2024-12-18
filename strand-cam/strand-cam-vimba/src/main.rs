use eyre::Result;

lazy_static::lazy_static! {
    static ref VIMBA_MODULE: ci2_vimba::WrappedModule = ci2_vimba::new_module().unwrap();
}

fn main() -> Result<()> {
    let guard = ci2_vimba::make_singleton_guard(&&*VIMBA_MODULE)?;
    let mymod = ci2_async::into_threaded_async(&*VIMBA_MODULE, &guard);
    strand_cam::cli_app::cli_main(mymod, env!("CARGO_PKG_NAME"))?;
    Ok(())
}
