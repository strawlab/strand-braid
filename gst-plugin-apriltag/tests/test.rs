const FNAME: &str = "movie-standard41h12.mkv";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "ddd2932d74139cd6ab5500b40c5f0482d5036df2f766be3a5f28ae2345e23aed";

#[test]
fn test_runs() {
    env_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    // Run ffmpeg over the file.
    let mut cmd = std::process::Command::new("gst-launch-1.0");
    cmd.arg("filesrc");
    cmd.arg(format!("location={}", &FNAME));
    cmd.arg("!");
    cmd.arg("decodebin");
    cmd.arg("!");
    cmd.arg("videoconvert");
    cmd.arg("!");
    cmd.arg("aptriltagdetector");
    cmd.arg("family=standard-41h12={}");
    cmd.arg("!");
    cmd.arg("filesink");
    cmd.arg("location=/dev/null");
    cmd.output().unwrap();
}
