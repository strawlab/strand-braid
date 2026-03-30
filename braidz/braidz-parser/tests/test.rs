const URL_BASE: &str = "https://strawlab-cdn.com/assets/";

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[test]
fn test_20201104_174158() {
    // This file was failing.
    const FILE1_FNAME: &str = "20201104_174158.braidz";
    const FILE1_SHA256SUM: &str =
        "d9e742336cf924f378e49055f3a709e52817ed90385c4f777f443952cf0557d6";

    init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FILE1_FNAME).as_str(),
        FILE1_FNAME,
        &download_verify::Hash::Sha256(FILE1_SHA256SUM.into()),
    )
    .unwrap();

    let attr = std::fs::metadata(FILE1_FNAME).unwrap();
    let archive = braidz_parser::braidz_parse_path(FILE1_FNAME).unwrap();
    let _summary = braidz_parser::summarize_braidz(&archive, FILE1_FNAME.to_string(), attr.len());
}

#[test]
fn test_20191125_104254() {
    // This file was failing.
    const FILE2_FNAME: &str = "20191125_104254.braidz";
    const FILE2_SHA256SUM: &str =
        "94086e464563416d55dce2615458c81834b63d99a91851afe84db8a8b57019d9";

    init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FILE2_FNAME).as_str(),
        FILE2_FNAME,
        &download_verify::Hash::Sha256(FILE2_SHA256SUM.into()),
    )
    .unwrap();

    let attr = std::fs::metadata(FILE2_FNAME).unwrap();
    let archive = braidz_parser::braidz_parse_path(FILE2_FNAME).unwrap();
    let _summary = braidz_parser::summarize_braidz(&archive, FILE2_FNAME.to_string(), attr.len());
}
