// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use imops::*;
use machine_vision_formats::pixel_format::Mono8;

fn get_im() -> machine_vision_formats::owned::OImage<Mono8> {
    const W: usize = 1024;
    const H: usize = 1024;
    let mut image_data = vec![0u8; W * H];
    image_data[4 * W + 3] = 1;
    image_data[5 * W + 3] = 1;
    image_data[5 * W + 4] = 1;
    image_data[6 * W + 4] = 1;

    machine_vision_formats::owned::OImage::new(W as u32, H as u32, W, image_data).unwrap()
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("theshold_less_than", |b| {
        let mut im = Some(get_im());
        b.iter(move || {
            im = Some(threshold(
                black_box(im.take().unwrap()),
                CmpOp::LessThan,
                10,
                0,
                255,
            ))
        });
    });

    c.bench_function("clip_low", |b| {
        let mut im = Some(get_im());
        b.iter(|| im = Some(clip_low(black_box(im.take().unwrap()), 10)));
    });

    c.bench_function("spatial_moment_00", |b| {
        let im = get_im();
        b.iter(|| spatial_moment_00(black_box(&im)));
    });

    c.bench_function("spatial_moment_10", |b| {
        let im = get_im();
        b.iter(|| spatial_moment_10(black_box(&im)));
    });

    c.bench_function("spatial_moment_01", |b| {
        let im = get_im();
        b.iter(|| spatial_moment_01(black_box(&im)));
    });

    // 60x60 feature window (feature_window_size=30 -> 60x60), as used by the
    // flydra feature detector. "sparse" mimics a despeckled blob; "dense" is
    // the worst case (every pixel nonzero).
    fn get_window(dense: bool) -> machine_vision_formats::owned::OImage<Mono8> {
        const W: usize = 60;
        let mut d = vec![0u8; W * W];
        if dense {
            for (i, p) in d.iter_mut().enumerate() {
                *p = ((i * 7) % 200) as u8;
            }
        } else {
            for dy in 0..6 {
                for dx in 0..6 {
                    d[(27 + dy) * W + (27 + dx)] = 200;
                }
            }
        }
        machine_vision_formats::owned::OImage::new(W as u32, W as u32, W, d).unwrap()
    }

    c.bench_function("calculate_moments_sparse_60x60", |b| {
        let im = get_window(false);
        b.iter(|| calculate_moments(black_box(&im)));
    });

    c.bench_function("calculate_moments_dense_60x60", |b| {
        let im = get_window(true);
        b.iter(|| calculate_moments(black_box(&im)));
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
