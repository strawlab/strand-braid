fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
