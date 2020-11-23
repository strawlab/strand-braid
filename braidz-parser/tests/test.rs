// This file was failing.
const FNAME: &str = "20201104_174158.braidz";
const URL_BASE: &str = "https://strawlab-cdn.com/assets/";
const SHA256SUM: &str = "d9e742336cf924f378e49055f3a709e52817ed90385c4f777f443952cf0557d6";

#[test]
fn test_20201104_174158() {
    env_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let attr = std::fs::metadata(&FNAME).unwrap();
    let archive = braidz_parser::braidz_parse_path(&FNAME).unwrap();
    let _summary = braidz_parser::summarize_braidz(&archive, FNAME.to_string(), attr.len());
}
