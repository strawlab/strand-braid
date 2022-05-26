fn main() -> Result<(), Box<(dyn std::error::Error)>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    build_util::bui_backend_generate_code("yew_frontend/pkg", "frontend.rs")?;
    Ok(())
}
