#![allow(dead_code)]

extern crate fastimage;
extern crate itertools;
#[macro_use]
extern crate criterion;

use criterion::Criterion;
use itertools::multizip;

use fastimage::{ipp_ctypes, ripp, Chan1, FastImage, FastImageData};

fn absdiff_8u_v2(img1: &[u8], img2: &[u8], output: &mut [u8]) {
    // see V2 of https://stackoverflow.com/a/35779655/1633026

    for (i1, i2, out) in multizip((img1.iter(), img2.iter(), output.iter_mut())) {
        *out = (*i1 as i16 - *i2 as i16).wrapping_abs() as u8;
    }
}

fn absdiff_8u_v6(img1: &[u8], img2: &[u8], output: &mut [u8]) {
    // see V6 of https://stackoverflow.com/a/35779655/1633026
    //     def differenceImageV6(img1, img2):
    //         a = img1-img2
    //         b = np.uint8(img1<img2) * 254 + 1
    //         return a * b

    for (i1, i2, out) in multizip((img1.iter(), img2.iter(), output.iter_mut())) {
        let a = i1.wrapping_sub(*i2);
        let b = (i1 < i2) as u8 * 254 + 1;
        *out = a.wrapping_mul(b);
    }
}

fn bench_abs_diff_8u_c1r(c: &mut Criterion) {
    ripp::init().unwrap();

    const W: ipp_ctypes::c_int = 1280;
    const H: ipp_ctypes::c_int = 1024;
    let im10 = FastImageData::<Chan1, u8>::new(W, H, 10).unwrap();
    let im9 = FastImageData::<Chan1, u8>::new(W, H, 9).unwrap();

    let mut im_dest = FastImageData::<Chan1, u8>::new(W, H, 0).unwrap();

    let size = *im_dest.size();
    c.bench_function("abs_diff_8u_c1r", move |b| {
        b.iter(|| ripp::abs_diff_8u_c1r(&im10, &im9, &mut im_dest, &size).unwrap())
    });
}

fn bench_abs_diff_simd(c: &mut Criterion) {
    #[cfg(feature = "simd-avx2")]
    use fastimage::simd_avx2 as simd;

    #[cfg(feature = "simd-sse2")]
    use fastimage::simd_sse2 as simd;

    const W: usize = 1280;
    const H: usize = 1024;
    let im10: Vec<u8> = [10; W * H].to_vec();
    let im9: Vec<u8> = [9; W * H].to_vec();
    let mut im_dest: Vec<u8> = [0; W * H].to_vec();
    c.bench_function("bench_abs_diff_simd", move |b| {
        b.iter(|| unsafe { simd::abs_diff_8u_c1r(&im10, &im9, &mut im_dest) })
    });
}

fn bench_abs_diff_naive_v2(c: &mut Criterion) {
    const W: usize = 1280;
    const H: usize = 1024;
    let im10: Vec<u8> = [10; W * H].to_vec();
    let im9: Vec<u8> = [9; W * H].to_vec();
    let mut im_dest: Vec<u8> = [0; W * H].to_vec();
    c.bench_function("bench_abs_diff_naive_v2", move |b| {
        b.iter(|| absdiff_8u_v2(&im10, &im9, &mut im_dest))
    });
}

fn bench_abs_diff_naive_v6(c: &mut Criterion) {
    const W: usize = 1280;
    const H: usize = 1024;
    let im10: Vec<u8> = [10; W * H].to_vec();
    let im9: Vec<u8> = [9; W * H].to_vec();
    let mut im_dest: Vec<u8> = [0; W * H].to_vec();
    c.bench_function("bench_abs_diff_naive_v6", move |b| {
        b.iter(|| absdiff_8u_v6(&im10, &im9, &mut im_dest))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    // targets = bench_abs_diff_simd, bench_abs_diff_8u_c1r, bench_abs_diff_naive_v2, bench_abs_diff_naive_v6
    targets = bench_abs_diff_simd, bench_abs_diff_8u_c1r
}

criterion_main!(benches);
