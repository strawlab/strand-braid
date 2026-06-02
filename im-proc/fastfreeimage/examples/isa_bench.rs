//! Benchmark of the per-frame full-image op sequence (DetectAbsDiff + use_cmp,
//! mask skipped). Build with default flags for the sandybridge floor, or with
//! RUSTFLAGS="-C target-cpu=native" to let `wide` use AVX2.

use fastfreeimage::{
    AlgorithmHint, CompareOp, FastImageData, FastImageSize, MomentState, MutableFastImage, ripp,
};

fn main() {
    const W: i32 = 1280;
    const H: i32 = 1024;
    const N: usize = 12000;
    const DIFF_THRESHOLD: u8 = 30;

    let size = FastImageSize::new(W, H);

    let mut raw = FastImageData::<u8>::new(W, H, 0).unwrap();
    let mut mean = FastImageData::<u8>::new(W, H, 0).unwrap();
    let mut cmp = FastImageData::<u8>::new(W, H, 0).unwrap();
    let fill = |img: &mut FastImageData<u8>, a: usize, b: usize, m: usize| {
        for (y, row) in img.valid_row_iter_mut(size).unwrap().enumerate() {
            for (x, px) in row.iter_mut().enumerate() {
                *px = ((x * a + y * b) % m) as u8;
            }
        }
    };
    fill(&mut raw, 7, 13, 256);
    fill(&mut mean, 3, 5, 256);
    fill(&mut cmp, 1, 1, 64);

    let mut absdiff = FastImageData::<u8>::new(W, H, 0).unwrap();
    let mut cmpdiff = FastImageData::<u8>::new(W, H, 0).unwrap();
    let _ = MomentState::new(AlgorithmHint::Fast).unwrap();

    let start = std::time::Instant::now();
    let mut sink = 0u64;
    for _ in 0..N {
        ripp::abs_diff_8u_c1r(&raw, &mean, &mut absdiff, size).unwrap();
        ripp::threshold_val_8u_c1ir(
            &mut cmp,
            size,
            DIFF_THRESHOLD,
            DIFF_THRESHOLD,
            CompareOp::Less,
        )
        .unwrap();
        ripp::sub_8u_c1rsfs(&cmp, &absdiff, &mut cmpdiff, size, 0).unwrap();
        let (v, loc) = ripp::max_indx_8u_c1r(&cmpdiff, size).unwrap();
        sink = sink.wrapping_add(v as u64 + loc.x() as u64 + loc.y() as u64);
    }
    let dur = start.elapsed();
    let per_frame_us = dur.as_secs_f64() * 1e6 / N as f64;
    println!("target features: {}", target_features());
    println!(
        "big-4 ops: {N} iters in {:.3} s -> {:.2} us/frame (sink={sink})",
        dur.as_secs_f64(),
        per_frame_us
    );

    // moments on a 60x60 feature window (feature_window_size=30 -> 60x60)
    let mut win = FastImageData::<u8>::new(60, 60, 0).unwrap();
    let wsize = FastImageSize::new(60, 60);
    for (y, row) in win.valid_row_iter_mut(wsize).unwrap().enumerate() {
        for (x, px) in row.iter_mut().enumerate() {
            *px = ((x * 7 + y * 11) % 200) as u8;
        }
    }
    let mut moments = MomentState::new(AlgorithmHint::Fast).unwrap();
    let origin = fastfreeimage::Point::new(0, 0);
    let start = std::time::Instant::now();
    let mut msink = 0f64;
    for _ in 0..N {
        ripp::moments_8u_c1r(&win, wsize, &mut moments).unwrap();
        msink += moments.spatial(0, 0, 0, &origin).unwrap()
            + moments.spatial(1, 0, 0, &origin).unwrap()
            + moments.spatial(0, 1, 0, &origin).unwrap()
            + moments.central(1, 1, 0).unwrap()
            + moments.central(2, 0, 0).unwrap()
            + moments.central(0, 2, 0).unwrap();
    }
    let mdur = start.elapsed();
    println!(
        "moments(60x60): {N} iters in {:.3} s -> {:.3} us/call (msink={msink:.1})",
        mdur.as_secs_f64(),
        mdur.as_secs_f64() * 1e6 / N as f64
    );
}

fn target_features() -> &'static str {
    if cfg!(target_feature = "avx512f") {
        "avx512f (and below)"
    } else if cfg!(target_feature = "avx2") {
        "avx2 (no avx512)"
    } else if cfg!(target_feature = "avx") {
        "avx (no avx2) — sandybridge"
    } else {
        "sse-only"
    }
}
