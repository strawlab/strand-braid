//! Measure IPP's per-frame full-image op cost at a forced instruction set.
//!
//! Set env `IPP_FORCE_ISA=sandybridge` (AVX, no AVX2/512) or `=avx2`, or leave
//! unset for the CPU's best (AVX-512 here). The op sequence mirrors the
//! flydra-feature-detector hot path (DetectAbsDiff + use_cmp, mask skipped).

use fastimage::{CompareOp, FastImageData, FastImageSize, MomentState, ripp};

fn name_of(version: *const ipp_sys::IppLibraryVersion) -> String {
    let inner = unsafe { *version };
    let name = unsafe { std::ffi::CStr::from_ptr(inner.Name) };
    name.to_string_lossy().into_owned()
}

fn main() {
    const W: i32 = 1280;
    const H: i32 = 1024;
    const N: usize = 12000;
    const DIFF_THRESHOLD: u8 = 30;

    // Force a CPU-feature subset before any IPP work, if requested.
    match std::env::var("IPP_FORCE_ISA").as_deref() {
        Ok("sandybridge") => {
            let mask = (ipp_sys::ippCPUID_MMX
                | ipp_sys::ippCPUID_SSE
                | ipp_sys::ippCPUID_SSE2
                | ipp_sys::ippCPUID_SSE3
                | ipp_sys::ippCPUID_SSSE3
                | ipp_sys::ippCPUID_SSE41
                | ipp_sys::ippCPUID_SSE42
                | ipp_sys::ippCPUID_AVX) as ipp_sys::Ipp64u;
            let st = unsafe { ipp_sys::ippSetCpuFeatures(mask) };
            println!("ippSetCpuFeatures(sandybridge) status = {st}");
        }
        Ok("avx2") => {
            let mask = (ipp_sys::ippCPUID_MMX
                | ipp_sys::ippCPUID_SSE
                | ipp_sys::ippCPUID_SSE2
                | ipp_sys::ippCPUID_SSE3
                | ipp_sys::ippCPUID_SSSE3
                | ipp_sys::ippCPUID_SSE41
                | ipp_sys::ippCPUID_SSE42
                | ipp_sys::ippCPUID_AVX
                | ipp_sys::ippCPUID_AVX2) as ipp_sys::Ipp64u;
            let st = unsafe { ipp_sys::ippSetCpuFeatures(mask) };
            println!("ippSetCpuFeatures(avx2) status = {st}");
        }
        _ => {
            let st = unsafe { ipp_sys::ippInit() };
            println!("ippInit() status = {st}");
        }
    }
    println!("ippi dispatch: {}", name_of(unsafe {
        ipp_sys::ippiGetLibVersion()
    }));

    let size = FastImageSize::new(W, H);

    // Stand-ins for the per-frame buffers. Use varied content so max_indx and
    // the arithmetic do real work (uniform images can be specially handled).
    let mut raw = FastImageData::<u8>::new(W, H, 0).unwrap();
    let mut mean = FastImageData::<u8>::new(W, H, 0).unwrap();
    let mut cmp = FastImageData::<u8>::new(W, H, 0).unwrap();
    let fill = |img: &mut FastImageData<u8>, a: usize, b: usize, m: usize| {
        use fastimage::MutableFastImage;
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
    let mut moments = MomentState::new(fastimage::AlgorithmHint::Fast).unwrap();
    let _ = &mut moments;

    let start = std::time::Instant::now();
    let mut sink = 0u64;
    for _ in 0..N {
        // absdiff = |raw - mean|
        ripp::abs_diff_8u_c1r(&raw, &mean, &mut absdiff, size).unwrap();
        // clip cmp to diff_threshold (use_cmp path)
        ripp::threshold_val_8u_c1ir(&mut cmp, size, DIFF_THRESHOLD, DIFF_THRESHOLD, CompareOp::Less)
            .unwrap();
        // cmpdiff = absdiff - cmp (saturating)
        ripp::sub_8u_c1rsfs(&cmp, &absdiff, &mut cmpdiff, size, 0).unwrap();
        // locate max
        let (v, loc) = ripp::max_indx_8u_c1r(&cmpdiff, size).unwrap();
        sink = sink.wrapping_add(v as u64 + loc.x() as u64 + loc.y() as u64);
    }
    let dur = start.elapsed();
    let per_frame_us = dur.as_secs_f64() * 1e6 / N as f64;
    println!(
        "big-4 ops: {N} iters in {:.3} s -> {:.2} us/frame (sink={sink})",
        dur.as_secs_f64(),
        per_frame_us
    );

    // moments on a 60x60 feature window
    use fastimage::MutableFastImage;
    let mut win = FastImageData::<u8>::new(60, 60, 0).unwrap();
    let wsize = FastImageSize::new(60, 60);
    for (y, row) in win.valid_row_iter_mut(wsize).unwrap().enumerate() {
        for (x, px) in row.iter_mut().enumerate() {
            *px = ((x * 7 + y * 11) % 200) as u8;
        }
    }
    let mut mom = MomentState::new(fastimage::AlgorithmHint::Fast).unwrap();
    let origin = fastimage::Point::new(0, 0);
    let start = std::time::Instant::now();
    let mut msink = 0f64;
    for _ in 0..N {
        ripp::moments_8u_c1r(&win, wsize, &mut mom).unwrap();
        msink += mom.spatial(0, 0, 0, &origin).unwrap()
            + mom.spatial(1, 0, 0, &origin).unwrap()
            + mom.spatial(0, 1, 0, &origin).unwrap()
            + mom.central(1, 1, 0).unwrap()
            + mom.central(2, 0, 0).unwrap()
            + mom.central(0, 2, 0).unwrap();
    }
    let mdur = start.elapsed();
    println!(
        "moments(60x60): {N} iters in {:.3} s -> {:.3} us/call (msink={msink:.1})",
        mdur.as_secs_f64(),
        mdur.as_secs_f64() * 1e6 / N as f64
    );
}
