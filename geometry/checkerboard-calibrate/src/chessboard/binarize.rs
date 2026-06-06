// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Binarization primitives used by the chessboard detector, ported to match
//! OpenCV's `equalizeHist` and `adaptiveThreshold(ADAPTIVE_THRESH_MEAN_C)`
//! exactly.

/// `saturate_cast<uchar>(float)` — round to nearest (ties to even, like
/// `cvRound`) and clamp to `[0, 255]`.
fn saturate_u8(x: f64) -> u8 {
    x.round_ties_even().clamp(0.0, 255.0) as u8
}

/// Port of OpenCV `equalizeHist` for 8-bit single-channel images.
///
/// Builds the cumulative-histogram lookup table exactly as OpenCV does: the
/// first occupied bin maps to 0, and the remainder is scaled by
/// `255 / (total - hist[first])`.
pub fn equalize_hist(src: &[u8]) -> Vec<u8> {
    let mut hist = [0u64; 256];
    for &v in src {
        hist[v as usize] += 1;
    }
    let total = src.len() as u64;

    // First non-empty bin.
    let mut first = 0usize;
    while first < 256 && hist[first] == 0 {
        first += 1;
    }
    if first == 256 {
        // Empty input.
        return Vec::new();
    }
    if hist[first] == total {
        // Single intensity: OpenCV fills the output with that intensity.
        return vec![first as u8; src.len()];
    }

    let scale = 255.0 / (total - hist[first]) as f64;
    let mut lut = [0u8; 256];
    lut[first] = 0;
    let mut sum = 0u64;
    for (i, &h) in hist.iter().enumerate().skip(first + 1) {
        sum += h;
        lut[i] = saturate_u8(sum as f64 * scale);
    }

    src.iter().map(|&v| lut[v as usize]).collect()
}

/// Port of OpenCV `adaptiveThreshold` with `ADAPTIVE_THRESH_MEAN_C` and
/// `THRESH_BINARY`.
///
/// For each pixel the local mean over a `block_size x block_size` neighborhood
/// is computed with a normalized box filter and `BORDER_REPLICATE` (so the
/// kernel area is always `block_size^2`). A pixel becomes `255` when
/// `src > mean - ceil(c)`, else `0`.
///
/// Panics if `block_size` is even or less than 3, matching OpenCV's
/// requirement of an odd window.
pub fn adaptive_threshold_mean(
    src: &[u8],
    width: usize,
    height: usize,
    block_size: usize,
    c: f64,
) -> Vec<u8> {
    assert_eq!(src.len(), width * height, "src length must be width*height");
    assert!(
        block_size >= 3 && block_size % 2 == 1,
        "block_size must be odd and >= 3"
    );

    let r = block_size / 2;
    let pw = width + 2 * r;
    let ph = height + 2 * r;

    // Replicate-padded integral image. `integral[(y+1)*(pw+1) + (x+1)]` is the
    // sum of padded pixels in `[0, x] x [0, y]`.
    let stride = pw + 1;
    let mut integral = vec![0i64; stride * (ph + 1)];
    for py in 0..ph {
        // Source row with replicate clamping.
        let sy = py.saturating_sub(r).min(height - 1);
        let mut row_sum = 0i64;
        for px in 0..pw {
            let sx = px.saturating_sub(r).min(width - 1);
            row_sum += src[sy * width + sx] as i64;
            integral[(py + 1) * stride + (px + 1)] = integral[py * stride + (px + 1)] + row_sum;
        }
    }

    let area = (block_size * block_size) as f64;
    let idelta = c.ceil() as i64; // THRESH_BINARY uses ceil(delta)

    let mut dst = vec![0u8; src.len()];
    for y in 0..height {
        for x in 0..width {
            // Window in padded coords: rows [y, y+block_size), cols [x, x+block_size).
            let y0 = y;
            let y1 = y + block_size;
            let x0 = x;
            let x1 = x + block_size;
            let sum = integral[y1 * stride + x1]
                - integral[y0 * stride + x1]
                - integral[y1 * stride + x0]
                + integral[y0 * stride + x0];
            let mean = saturate_u8(sum as f64 / area) as i64;
            let v = src[y * width + x] as i64;
            dst[y * width + x] = if v > mean - idelta { 255 } else { 0 };
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equalize_hist_known_lut() {
        // Four distinct intensities, one pixel each.
        // first=0, hist[0]=1, total=4, scale=255/3=85.
        // lut = [0, 85, 170, 255].
        let src = [0u8, 1, 2, 3];
        assert_eq!(equalize_hist(&src), vec![0, 85, 170, 255]);
    }

    #[test]
    fn equalize_hist_constant_image() {
        let src = [42u8; 10];
        assert_eq!(equalize_hist(&src), vec![42u8; 10]);
    }

    #[test]
    fn equalize_hist_spreads_to_full_range() {
        // A clustered histogram should map min->0 and max->255.
        let src = [100u8, 100, 100, 150, 200];
        let out = equalize_hist(&src);
        assert_eq!(out[0], 0);
        assert_eq!(*out.last().unwrap(), 255);
        // Monotonic in intensity.
        assert!(out[3] <= out[4]);
    }

    #[test]
    fn adaptive_threshold_constant_image() {
        let src = [100u8; 9];
        // c = 0 -> idelta 0 -> v > mean is false everywhere -> all black.
        assert_eq!(adaptive_threshold_mean(&src, 3, 3, 3, 0.0), vec![0u8; 9]);
        // c = 1 -> idelta 1 -> 100 > 99 true -> all white.
        assert_eq!(adaptive_threshold_mean(&src, 3, 3, 3, 1.0), vec![255u8; 9]);
    }

    #[test]
    fn adaptive_threshold_bright_pixel() {
        // Center pixel much brighter than its neighborhood becomes white; the
        // dark surround stays black (with c = 0).
        #[rustfmt::skip]
        let src = [
            10, 10, 10,
            10, 250, 10,
            10, 10, 10u8,
        ];
        let out = adaptive_threshold_mean(&src, 3, 3, 3, 0.0);
        // mean of the 3x3 replicate-padded window for the center ~ (8*10+250)/9
        // = 36.7 -> 37; 250 > 37 -> white.
        assert_eq!(out[4], 255);
        // A corner pixel's neighborhood is dominated by 10s -> mean ~ small,
        // 10 > mean is false (10 ~ mean) -> black.
        assert_eq!(out[0], 0);
    }
}
