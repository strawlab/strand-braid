fn main() -> std::result::Result<(), anyhow::Error> {
    let vimba_module = ci2_vimba::new_module()?;
    let mymod = ci2_async::into_threaded_async(&vimba_module);
    strand_cam::cli_app::cli_main(mymod, env!("CARGO_PKG_NAME"))?;
    Ok(())
}
