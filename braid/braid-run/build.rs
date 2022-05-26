fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    build_util::bui_backend_generate_code("braid_frontend/pkg", "mainbrain_frontend.rs")?;
    Ok(())
}
