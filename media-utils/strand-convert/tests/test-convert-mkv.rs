// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::Path;
use tracing_test::traced_test;

use eyre::Result;
use frame_source::FrameDataSource;
use strand_cam_remote_control::H264Metadata;

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

fn get_metadata<P: AsRef<Path>>(fname: P) -> Result<H264Metadata> {
    let input_ext = fname.as_ref().extension().and_then(|x| x.to_str());
    match input_ext {
        Some("mkv") => {
            let mkv_video = frame_source::FrameSourceBuilder::new(&fname)
                .do_decode_h264(false)
                .build_mkv_source()?;
            let metadata = &mkv_video.parsed.metadata;
            let camera_name = metadata.camera_name.clone();
            let gamma = metadata.gamma;
            let creation_time = mkv_video.frame0_time().unwrap();
            Ok(H264Metadata {
                version: strand_cam_remote_control::H264_METADATA_VERSION.into(),
                writing_app: metadata.writing_app.clone(),
                camera_name,
                gamma,
                creation_time,
            })
        }
        Some("mp4") => {
            let mp4_video = frame_source::FrameSourceBuilder::new(&fname)
                .do_decode_h264(false)
                .build_h264_in_mp4_source()?;
            Ok(mp4_video.h264_metadata.unwrap())
        }
        ext => {
            todo!("unsuported extension {ext:?}");
        }
    }
}

/// The names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

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
        args.push("rescale-linear-to-8bits");
        args.push("--hdr-autodetect-range");
    }
    let cli = clap::Parser::try_parse_from(&args)?;
    strand_convert::run_cli(cli)?;

    // The movie is written under a temporary name beside its destination. On
    // success only the destination may remain.
    assert_eq!(names_in(outdir.path()), ["output.mp4"]);

    // // Actually test output of mp4
    // assert!(are_equivalent(&fname, &outfile)?);

    let input_ext = fname.as_ref().extension().and_then(|x| x.to_str());
    match input_ext {
        Some("mp4") | Some("mkv") => {
            // check metadata
            let input_md = get_metadata(&fname)?;
            let mut output_md = get_metadata(&outfile)?;
            output_md.writing_app = input_md.writing_app.clone(); // this may have changed
            assert_eq!(input_md, output_md);
        }
        _ => {}
    }

    if test_size {
        // Test that the mp4 is no more than 5% larger.
        let mkv_attr = std::fs::metadata(&fname)?;
        let mp4_attr = std::fs::metadata(&outfile)?;
        assert!(mp4_attr.len() as f64 <= mkv_attr.len() as f64 * 1.05); // must be no more than 5% larger
    }

    Ok(outdir)
}

#[traced_test]
#[test]
fn mkv_color_nvenc_h264() -> Result<()> {
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C003E00.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "7f7cd84fb8b5934e34e03c875e6a1da0d1ef3737f125de0f3b586a0451e58885";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(&dest, false, true)?;
    Ok(())
}

#[traced_test]
#[test]
fn mkv_mono_nvenc_h264() -> Result<()> {
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "9137122026736c719b897260c426d2e4337092aacc218ebe16d79470b0be3729";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(&dest, false, true)?;
    Ok(())
}

#[traced_test]
#[test]
fn mkv_mono_uncompressed() -> Result<()> {
    const FNAME: &str = "movie20221123_115611.593125675_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/uncompressed";
    const SHA256SUM: &str = "0cbe7a9c7a7be151dc8c401eb59e3fcd7d3589636bc70d0968f21673f8c95e45";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(&dest, false, true)?;
    Ok(())
}

#[traced_test]
#[test]
fn mp4_color_nvenc_h264() -> Result<()> {
    // first convert mkv -> mp4
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C003E00.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "7f7cd84fb8b5934e34e03c875e6a1da0d1ef3737f125de0f3b586a0451e58885";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let mp4dir = do_convert(&dest, false, true)?;

    // this is our mp4 file
    let mp4file = mp4dir.path().join("output.mp4");

    // now test mp4 -> mp4
    do_convert(mp4file, false, true)?;
    Ok(())
}

#[traced_test]
#[test]
fn mp4_mono_nvenc_h264() -> Result<()> {
    // We test mp4 by first making an mp4 from mkv. This also tests that the mp4
    // created from mkv can be used as a source for further conversions and thus
    // that the mkv->mp4 conversion preserves required data.

    // So, first convert mkv -> mp4.
    const FNAME: &str = "movie20221123_115306.150434017_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/color_mono";
    const SHA256SUM: &str = "9137122026736c719b897260c426d2e4337092aacc218ebe16d79470b0be3729";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let mp4dir = do_convert(&dest, false, true)?;

    // this is our mp4 file
    let mp4file = mp4dir.path().join("output.mp4");

    // now test mp4 -> mp4
    do_convert(mp4file, false, true)?;
    Ok(())
}

#[traced_test]
#[test]
fn tiff_12bit_mono() -> Result<()> {
    const FNAME: &str = "20221103_test.zip";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/photometrics-samples";
    const SHA256SUM: &str = "41bc89f2735250e02e308ff65009ad110888a57781a89de5b40b0033b20be483";

    let dest = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let outdir = tempfile::tempdir().unwrap(); // will cleanup on drop
    let one_default = outdir.path().join("_1").join("Default");
    std::fs::create_dir_all(&one_default)?;
    let mut zip_archive = zip::ZipArchive::new(std::fs::File::open(&dest)?)?;
    zip_archive.extract(&one_default)?;

    do_convert(&one_default, true, false)?;
    Ok(())
}

#[traced_test]
#[test]
fn fmf_gz_mono() -> Result<()> {
    // first convert fmf -> mp4
    const FNAME: &str = "movie20211109_080701_Basler-21714402.fmf.gz";
    let local_fname = format!("scratch/{}", FNAME);
    const URL_BASE: &str =
        "https://strawlab-cdn.com/assets/flycube6-videos/fc6-led-4fps-5-cams-bright";
    const SHA256SUM: &str = "fa1ef64b4ab967fd081ab3f026805662212e6b7696a52d1ccc06b61703c3c467";

    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    do_convert(&local_fname, false, false)?;
    Ok(())
}

/// A conversion which fails after it has started writing must leave nothing at
/// its destination: a movie cut short there would pass for a finished one.
/// With `--truncate-on-error`, what was written is kept.
#[traced_test]
#[test]
fn failed_conversion_leaves_no_output() -> Result<()> {
    const FNAME: &str = "movie20211109_080701_Basler-21714402.fmf.gz";
    let local_fname = format!("scratch/{}", FNAME);
    const URL_BASE: &str =
        "https://strawlab-cdn.com/assets/flycube6-videos/fc6-led-4fps-5-cams-bright";
    const SHA256SUM: &str = "fa1ef64b4ab967fd081ab3f026805662212e6b7696a52d1ccc06b61703c3c467";

    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    // An FMF file holds no H264 stream to copy. That is found out only when
    // the first frame is to be written, after the output has been created.
    let convert = |extra_arg: Option<&str>| -> Result<Vec<String>> {
        let outdir = tempfile::tempdir()?;
        let outfile = outdir.path().join("output.mp4");
        let mut args = vec![
            "strand-convert",
            "-i",
            &local_fname,
            "-o",
            outfile.to_str().unwrap(),
            "--no-progress",
            "--encoder",
            "none-copy-existing-h264",
        ];
        args.extend(extra_arg);
        let cli = clap::Parser::try_parse_from(&args)?;
        assert!(strand_convert::run_cli(cli).is_err());
        Ok(names_in(outdir.path()))
    };

    assert!(convert(None)?.is_empty());
    assert_eq!(convert(Some("--truncate-on-error"))?, ["output.mp4"]);
    Ok(())
}

/// The number of frames in an MP4 file which carry a capture time.
fn count_timestamped_frames(mp4: &Path) -> Result<usize> {
    let mut src = frame_source::FrameSourceBuilder::new(mp4)
        .do_decode_h264(false)
        .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
        .build_h264_in_mp4_source()?;
    let mut count = 0;
    for frame in src.decode_order_iter() {
        frame?;
        count += 1;
    }
    Ok(count)
}

/// Encoding with ffmpeg gives the same frames, each with its capture time, and
/// the metadata, as encoding with a built-in encoder does, and leaves none of
/// the files it works with behind.
#[traced_test]
#[test]
fn mkv_mono_uncompressed_with_ffmpeg() -> Result<()> {
    // libx264 wants even dimensions for 4:2:0 video, which this recording has.
    const FNAME: &str = "movie20221123_115611.593125675_DEV_1AB22C00E48D.mkv";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/braid-mkvs/uncompressed";
    const SHA256SUM: &str = "0cbe7a9c7a7be151dc8c401eb59e3fcd7d3589636bc70d0968f21673f8c95e45";

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let convert = |extra_args: &[&str]| -> Result<tempfile::TempDir> {
        let outdir = tempfile::tempdir()?;
        let outfile = outdir.path().join("output.mp4");
        let mut args = vec![
            "strand-convert",
            "-i",
            &local_fname,
            "-o",
            outfile.to_str().unwrap(),
            "--no-progress",
        ];
        args.extend(extra_args);
        strand_convert::run_cli(clap::Parser::try_parse_from(&args)?)?;
        assert_eq!(names_in(outdir.path()), ["output.mp4"]);
        Ok(outdir)
    };

    let built_in = convert(&[])?;
    let ffmpeg = convert(&["--encoder", "ffmpeg"])?;

    let built_in = built_in.path().join("output.mp4");
    let ffmpeg = ffmpeg.path().join("output.mp4");
    let n_frames = count_timestamped_frames(&built_in)?;
    assert!(n_frames > 0);
    assert_eq!(count_timestamped_frames(&ffmpeg)?, n_frames);

    let mut ffmpeg_md = get_metadata(&ffmpeg)?;
    let built_in_md = get_metadata(&built_in)?;
    ffmpeg_md.writing_app = built_in_md.writing_app.clone(); // names the encoder
    assert_eq!(ffmpeg_md, built_in_md);
    Ok(())
}

/// A conversion with ffmpeg which fails leaves nothing behind either.
#[traced_test]
#[test]
fn failed_ffmpeg_conversion_leaves_no_output() -> Result<()> {
    const FNAME: &str = "movie20211109_080701_Basler-21714402.fmf.gz";
    let local_fname = format!("scratch/{}", FNAME);
    const URL_BASE: &str =
        "https://strawlab-cdn.com/assets/flycube6-videos/fc6-led-4fps-5-cams-bright";
    const SHA256SUM: &str = "fa1ef64b4ab967fd081ab3f026805662212e6b7696a52d1ccc06b61703c3c467";

    download_verify::download_verify(
        format!("{URL_BASE}/{FNAME}").as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let outdir = tempfile::tempdir()?;
    let outfile = outdir.path().join("output.mp4");
    // ffmpeg starts, and then finds that it has no such encoder.
    let args = [
        "strand-convert",
        "-i",
        &local_fname,
        "-o",
        outfile.to_str().unwrap(),
        "--no-progress",
        "--encoder",
        "ffmpeg",
        "--ffmpeg-codec",
        "no-such-codec",
    ];
    assert!(strand_convert::run_cli(clap::Parser::try_parse_from(args)?).is_err());
    assert!(names_in(outdir.path()).is_empty());
    Ok(())
}
