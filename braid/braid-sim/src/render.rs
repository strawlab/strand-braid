// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Render insects as Gaussian blobs into a Mono8 image, matching what the M0
//! detector-contract spike validated.

/// Render a `width` x `height` Mono8 image: a flat `background` with an additive
/// Gaussian blob (peak `peak`, standard deviation `sigma` pixels) at each
/// `(x, y)` center in `blobs`. Stride equals `width`.
///
/// Only a window of a few sigma around each blob is touched, so this is cheap
/// even for large images.
pub fn render_mono8(
    width: usize,
    height: usize,
    background: u8,
    blobs: &[(f64, f64)],
    peak: f64,
    sigma: f64,
) -> Vec<u8> {
    let mut buf = vec![background; width * height];
    if peak <= 0.0 || sigma <= 0.0 {
        return buf;
    }
    let two_sig2 = 2.0 * sigma * sigma;
    let radius = (4.0 * sigma).ceil() as i64;
    for &(cx, cy) in blobs {
        let cxr = cx.round() as i64;
        let cyr = cy.round() as i64;
        let y0 = (cyr - radius).max(0);
        let y1 = (cyr + radius + 1).min(height as i64);
        let x0 = (cxr - radius).max(0);
        let x1 = (cxr + radius + 1).min(width as i64);
        for py in y0..y1 {
            for px in x0..x1 {
                let dx = px as f64 - cx;
                let dy = py as f64 - cy;
                let g = peak * (-(dx * dx + dy * dy) / two_sig2).exp();
                let idx = py as usize * width + px as usize;
                let v = (buf[idx] as f64 + g).round().clamp(0.0, 255.0) as u8;
                buf[idx] = v;
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_is_brightest_at_center_and_background_far_away() {
        let (w, h) = (64usize, 48usize);
        let buf = render_mono8(w, h, 0, &[(20.0, 15.0)], 160.0, 1.5);
        let at = |x: usize, y: usize| buf[y * w + x] as u32;
        // Peak at the center, dark in a far corner.
        assert_eq!(at(20, 15), 160);
        assert_eq!(at(60, 45), 0);
        // Monotonic falloff moving away from center.
        assert!(at(20, 15) > at(22, 15));
        assert!(at(22, 15) > at(25, 15));
    }

    #[test]
    fn empty_blobs_yield_flat_background() {
        let buf = render_mono8(8, 8, 7, &[], 160.0, 1.5);
        assert!(buf.iter().all(|&v| v == 7));
    }
}
