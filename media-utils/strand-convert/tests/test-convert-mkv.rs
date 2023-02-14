use std::path::Path;

use anyhow::Result;

// use frame_source::{EncodedH264, ImageData};

// fn decode_h264_image(e: &EncodedH264) -> Result<u8> {
//     println!("{:?}", &e.data[..300]);
//     todo!();
//     Ok(213)
// }

// fn is_image_data_exactly_equal(self_: &ImageData, other: &ImageData) -> Result<bool> {
//     match (self_, other) {
//         (ImageData::Decoded(d1), ImageData::Decoded(d2)) => Ok(d1 == d2),
//         (ImageData::Tiff(t1), ImageData::Tiff(t2)) => {
//             todo!();
//         }
//         (ImageData::EncodedH264(e1), ImageData::EncodedH264(e2)) => {
//             let d1 = decode_h264_image(e1)?;
//             let d2 = decode_h264_image(e2)?;
//             Ok(d1 == d2)
//         }
//         _ => Ok(false),
//     }
// }

// fn are_equivalent<P1, P2>(fname1: P1, fname2: P2) -> Result<bool>
// where
//     P1: AsRef<Path>,
//     P2: AsRef<Path>,
// {
//     dbg!(fname1.as_ref().display());
//     dbg!(fname2.as_ref().display());
//     let mut src1 = frame_source::from_path(fname1)?;
//     let mut src2 = frame_source::from_path(fname2)?;
//     for (f1, f2) in src1.iter().zip(src2.iter()) {
//         let f1 = f1?;
//         let f2 = f2?;
//         dbg!(&f1);
//         dbg!(&f2);
//         println!();
//         if f1.timestamp() != f2.timestamp() {
//             return Ok(false);
//         }
//         if f1.idx() != f2.idx() {
//             return Ok(false);
//         }
//         if !is_image_data_exactly_equal(f1.image(), f2.image())? {
//             return Ok(false);
//         }
//     }
//     Ok(true)
// }

fn do_convert<P: AsRef<Path>>(
    fname: P,
    autoscale_hdr: bool,
    test_size: bool,
) -> Result<tempfile::TempDir> {
    let fname_str = format!("{}", fname.as_ref().display());
    let outdir = tempfile::tempdir().unwrap(); // will cleanup on drop
    let outfile = outdir.path().join("output.mp4");
    let mut args = vec![
        "strand-convert",
        "-i",
        &fname_str,
        "-o",
        outfile.to_str().unwrap(),
        "--no-progress",
    ];
    if autoscale_hdr {
        args.push("--hdr-config");
        args.push("recale-linear-to-8bits");
        args.push("--hdr-autodetect-range");
    }
    let cli = clap::Parser::try_parse_from(&args)?;
    strand_convert::run_cli(cli)?;

    // // Actually test output of mp4
    // assert!(are_equivalent(&fname, &outfile)?);

    if test_size {
        // Test that the mp4 is no more than 5% larger.
        let mkv_attr = std::fs::metadata(&fname)?;
        let mp4_attr = std::fs::metadata(&outfile)?;
        assert!(mp4_attr.len() as f64 <= mkv_attr.len() as f64 * 1.05); // must be no more than 5% larger
    }

    Ok(outdir)
}

#[test]
fn mkv_color_nvenc_h264() -> Result<()> {
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C003E00.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "7f7cd84fb8b5934e34e03c875e6a1da0d1ef3737f125de0f3b586a0451e58885";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(FNAME, false, true)?;
    Ok(())
}

#[test]
fn mkv_mono_nvenc_h264() -> Result<()> {
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "9137122026736c719b897260c426d2e4337092aacc218ebe16d79470b0be3729";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(FNAME, false, true)?;
    Ok(())
}

#[test]
fn mkv_mono_uncompressed() -> Result<()> {
    const FNAME: &str = "movie20221123_115611.593125675_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/uncompressed";
    const SHA256SUM: &str = "0cbe7a9c7a7be151dc8c401eb59e3fcd7d3589636bc70d0968f21673f8c95e45";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(FNAME, false, true)?;
    Ok(())
}

#[test]
fn mp4_color_nvenc_h264() -> Result<()> {
    // first convert mkv -> mp4
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C003E00.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "7f7cd84fb8b5934e34e03c875e6a1da0d1ef3737f125de0f3b586a0451e58885";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let mp4dir = do_convert(FNAME, false, true)?;

    // this is our mp4 file
    let mp4file = mp4dir.path().join("output.mp4");

    // now test mp4 -> mp4
    do_convert(mp4file, false, true)?;
    Ok(())
}

#[test]
fn mp4_mono_nvenc_h264() -> Result<()> {
    // We test mp4 by first making an mp4 from mkv. This also tests that the mp4
    // created from mkv can be used as a source for further conversions and thus
    // that the mkv->mp4 conversion preserves required data.

    // So, first convert mkv -> mp4.
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "9137122026736c719b897260c426d2e4337092aacc218ebe16d79470b0be3729";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let mp4dir = do_convert(FNAME, false, true)?;

    // this is our mp4 file
    let mp4file = mp4dir.path().join("output.mp4");

    // now test mp4 -> mp4
    do_convert(mp4file, false, true)?;
    Ok(())
}

#[test]
fn tiff_12bit_mono() -> Result<()> {
    const FNAME: &str = "20221103_test.zip";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/photometrics-samples";
    const SHA256SUM: &str = "41bc89f2735250e02e308ff65009ad110888a57781a89de5b40b0033b20be483";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let outdir = tempfile::tempdir().unwrap(); // will cleanup on drop
    let one_default = outdir.path().join("_1").join("Default");
    std::fs::create_dir_all(&one_default)?;
    let mut zip_archive = zip::ZipArchive::new(std::fs::File::open(FNAME)?)?;
    zip_archive.extract(&one_default)?;

    do_convert(&one_default, true, false)?;
    Ok(())
}

#[test]
fn fmf_gz_mono() -> Result<()> {
    // first convert fmf -> mp4
    const FNAME: &str = "movie20211109_080701_Basler-21714402.fmf.gz";
    const URL_BASE: &str =
        "https://strawlab-cdn.com/assets/flycube6-videos/fc6-led-4fps-5-cams-bright";
    const SHA256SUM: &str = "fa1ef64b4ab967fd081ab3f026805662212e6b7696a52d1ccc06b61703c3c467";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(FNAME, false, false)?;
    Ok(())
}
