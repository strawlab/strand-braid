extern crate bui_backend_codegen;

#[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
fn _compile_time_feature_test() {
    // Intentionally trigger a compile time error to force a feature
    // flag to be used.
    compile_error!("You are attempting to compile without a required feature flag \
    being used. You must use one of either `bundle_files` or `serve_files`");
}

fn main() {
    let files_dir: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "yew_frontend", "dist"].iter().collect();
    bui_backend_codegen::codegen(&files_dir, "rt-image-viewer-frontend.rs").expect("codegen failed");
}
