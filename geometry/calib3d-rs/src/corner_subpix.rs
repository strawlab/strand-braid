//! Pure-Rust port of OpenCV's `cv::cornerSubPix`.
//!
//! Given approximate corner locations and a grayscale image, this iteratively
//! refines each corner to sub-pixel accuracy by exploiting the fact that, at a
//! true corner, the image gradient at every nearby pixel is orthogonal to the
//! vector from the corner to that pixel. Each iteration solves the resulting
//! 2x2 weighted least-squares system for the corner position.
//!
//! The algorithm mirrors OpenCV's `modules/imgproc/src/cornersubpix.cpp` so the
//! results match within a small tolerance (see the cross-check tests in the
//! `opencv-calibrate` crate). Notable fidelity points:
//!
//! - The window weight is the separable Gaussian
//!   `exp(-x^2) * exp(-y^2)` with `x = (col - win_w)/win_w`,
//!   `y = (row - win_h)/win_h`, matching OpenCV exactly.
//! - Image values are sampled with bilinear interpolation and
//!   `BORDER_REPLICATE`, matching `getRectSubPix`.
//! - Iteration stops on either the max-count or the EPS criterion (squared
//!   movement), and a result that drifts more than one half-window from the
//!   start is reverted to the start, as OpenCV does.

/// A borrowed 8-bit grayscale image (row-major, one byte per pixel).
#[derive(Clone, Copy)]
pub struct GrayImageRef<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl<'a> GrayImageRef<'a> {
    pub fn new(data: &'a [u8], width: usize, height: usize) -> Self {
        assert_eq!(
            data.len(),
            width * height,
            "data length must be width*height"
        );
        Self {
            data,
            width,
            height,
        }
    }

    /// Bilinear sample with `BORDER_REPLICATE`, matching OpenCV `getRectSubPix`.
    fn sample(&self, x: f64, y: f64) -> f64 {
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;

        let x0 = self.clamp_x(ix as i64);
        let x1 = self.clamp_x(ix as i64 + 1);
        let y0 = self.clamp_y(iy as i64);
        let y1 = self.clamp_y(iy as i64 + 1);

        let p00 = self.data[y0 * self.width + x0] as f64;
        let p01 = self.data[y0 * self.width + x1] as f64;
        let p10 = self.data[y1 * self.width + x0] as f64;
        let p11 = self.data[y1 * self.width + x1] as f64;

        let top = p00 * (1.0 - fx) + p01 * fx;
        let bot = p10 * (1.0 - fx) + p11 * fx;
        top * (1.0 - fy) + bot * fy
    }

    fn clamp_x(&self, v: i64) -> usize {
        v.clamp(0, self.width as i64 - 1) as usize
    }

    fn clamp_y(&self, v: i64) -> usize {
        v.clamp(0, self.height as i64 - 1) as usize
    }
}

/// Parameters for [`corner_subpix`], mirroring OpenCV's arguments.
#[derive(Clone, Copy, Debug)]
pub struct CornerSubPixParams {
    /// Half-size of the search window: full window is `2*win_half + 1` per axis.
    pub win_half: (usize, usize),
    /// Half-size of a central "dead zone" excluded from the sums (used to avoid
    /// a singular autocorrelation matrix at the exact center). `None` disables
    /// it (OpenCV's `Size(-1, -1)`).
    pub zero_zone_half: Option<(usize, usize)>,
    /// Maximum number of refinement iterations per corner.
    pub max_count: usize,
    /// Convergence threshold on the corner movement, in pixels.
    pub eps: f64,
}

impl Default for CornerSubPixParams {
    /// The settings strand-braid uses for chessboard refinement:
    /// `win = (11, 11)`, no zero-zone, `maxCount = 30`, `eps = 0.1`.
    fn default() -> Self {
        Self {
            win_half: (11, 11),
            zero_zone_half: None,
            max_count: 30,
            eps: 0.1,
        }
    }
}

/// Build the separable Gaussian window mask, identical to OpenCV's.
fn build_mask(win_half: (usize, usize), zero_zone_half: Option<(usize, usize)>) -> Vec<f64> {
    let (whw, whh) = win_half;
    let win_w = whw * 2 + 1;
    let win_h = whh * 2 + 1;
    let mut mask = vec![0.0f64; win_w * win_h];

    for i in 0..win_h {
        let y = (i as f64 - whh as f64) / whh as f64;
        let vy = (-y * y).exp();
        for j in 0..win_w {
            let x = (j as f64 - whw as f64) / whw as f64;
            mask[i * win_w + j] = vy * (-x * x).exp();
        }
    }

    if let Some((zw, zh)) = zero_zone_half {
        for i in whh.saturating_sub(zh)..=(whh + zh).min(win_h - 1) {
            for j in whw.saturating_sub(zw)..=(whw + zw).min(win_w - 1) {
                mask[i * win_w + j] = 0.0;
            }
        }
    }

    mask
}

/// Refine `corners` to sub-pixel accuracy. Returns refined copies in the same
/// order; corners are independent of one another.
pub fn corner_subpix(
    img: GrayImageRef,
    corners: &[(f32, f32)],
    params: &CornerSubPixParams,
) -> Vec<(f32, f32)> {
    let (whw, whh) = params.win_half;
    let win_w = whw * 2 + 1;
    let win_h = whh * 2 + 1;
    let mask = build_mask(params.win_half, params.zero_zone_half);

    let eps2 = params.eps * params.eps;
    let det_thresh = f64::EPSILON * f64::EPSILON;

    corners
        .iter()
        .map(|&(ct_x, ct_y)| {
            let (ct_x, ct_y) = (ct_x as f64, ct_y as f64);
            let mut c_x = ct_x;
            let mut c_y = ct_y;

            let mut iter = 0;
            loop {
                let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
                let (mut bb1, mut bb2) = (0.0, 0.0);

                for i in 0..win_h {
                    let py = i as f64 - whh as f64;
                    for j in 0..win_w {
                        let px = j as f64 - whw as f64;
                        let m = mask[i * win_w + j];
                        if m == 0.0 {
                            continue;
                        }
                        // Central differences on the bilinearly-sampled image,
                        // at sub-pixel offset (px, py) from the current corner.
                        let tgx = img.sample(c_x + px + 1.0, c_y + py)
                            - img.sample(c_x + px - 1.0, c_y + py);
                        let tgy = img.sample(c_x + px, c_y + py + 1.0)
                            - img.sample(c_x + px, c_y + py - 1.0);

                        let gxx = tgx * tgx * m;
                        let gxy = tgx * tgy * m;
                        let gyy = tgy * tgy * m;

                        a += gxx;
                        b += gxy;
                        c += gyy;
                        bb1 += gxx * px + gxy * py;
                        bb2 += gxy * px + gyy * py;
                    }
                }

                let det = a * c - b * b;
                let (new_x, new_y) = if det.abs() > det_thresh {
                    let scale = 1.0 / det;
                    (
                        c_x + (c * bb1 - b * bb2) * scale,
                        c_y + (a * bb2 - b * bb1) * scale,
                    )
                } else {
                    (c_x, c_y)
                };

                let err = (new_x - c_x) * (new_x - c_x) + (new_y - c_y) * (new_y - c_y);
                c_x = new_x;
                c_y = new_y;

                iter += 1;
                let out_of_bounds =
                    c_x < 0.0 || c_x >= img.width as f64 || c_y < 0.0 || c_y >= img.height as f64;
                if out_of_bounds || iter >= params.max_count || err <= eps2 {
                    break;
                }
            }

            // Poor convergence: a corner that wandered more than one half-window
            // from its start is rejected (kept at the initial location).
            if (c_x - ct_x).abs() > whw as f64 || (c_y - ct_y).abs() > whh as f64 {
                (ct_x as f32, ct_y as f32)
            } else {
                (c_x as f32, c_y as f32)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a black/white checkerboard corner: the two step edges sit between
    /// columns `bx-1,bx` and rows `by-1,by`, so by symmetry the true sub-pixel
    /// corner (where `cornerSubPix`'s model is satisfied) is at
    /// `(bx - 0.5, by - 0.5)`.
    fn render_checker_corner(w: usize, h: usize, bx: usize, by: usize) -> Vec<u8> {
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let on = (x >= bx) == (y >= by);
                data[y * w + x] = if on { 255 } else { 0 };
            }
        }
        data
    }

    #[test]
    fn converges_to_checker_corner() {
        let (w, h) = (41, 41);
        let (bx, by) = (20usize, 15usize);
        let (cx, cy) = (bx as f64 - 0.5, by as f64 - 0.5); // true corner (19.5, 14.5)
        let data = render_checker_corner(w, h, bx, by);
        let img = GrayImageRef::new(&data, w, h);

        let params = CornerSubPixParams {
            win_half: (5, 5),
            zero_zone_half: None,
            max_count: 40,
            eps: 1e-4,
        };

        let start = [(18.0_f32, 16.0_f32)];
        let refined = corner_subpix(img, &start, &params);

        let (rx, ry) = refined[0];
        let start_err = (start[0].0 as f64 - cx).hypot(start[0].1 as f64 - cy);
        let refined_err = (rx as f64 - cx).hypot(ry as f64 - cy);

        assert!(
            refined_err < start_err,
            "refinement should improve: start {start_err:.3} -> refined {refined_err:.3}"
        );
        assert!(
            refined_err < 0.2,
            "refined corner ({rx:.3},{ry:.3}) not within 0.2px of true ({cx},{cy}); err={refined_err:.3}"
        );
    }

    #[test]
    fn true_corner_is_a_fixed_point() {
        // Starting exactly at the symmetric crossing, the result must not drift.
        let (w, h) = (41, 41);
        let (bx, by) = (20usize, 15usize);
        let (cx, cy) = (bx as f64 - 0.5, by as f64 - 0.5);
        let data = render_checker_corner(w, h, bx, by);
        let img = GrayImageRef::new(&data, w, h);

        let start = [(cx as f32, cy as f32)];
        let refined = corner_subpix(img, &start, &CornerSubPixParams::default());
        let (rx, ry) = refined[0];
        assert!(
            (rx as f64 - cx).hypot(ry as f64 - cy) < 1e-3,
            "fixed point drifted to ({rx},{ry})"
        );
    }

    #[test]
    fn flat_region_is_a_no_op() {
        // No gradients => singular system => corner must not move.
        let (w, h) = (31, 31);
        let data = vec![100u8; w * h];
        let img = GrayImageRef::new(&data, w, h);

        let start = [(15.0_f32, 15.0_f32)];
        let refined = corner_subpix(img, &start, &CornerSubPixParams::default());
        assert_eq!(refined[0], (15.0, 15.0));
    }

    #[test]
    fn mask_matches_opencv_formula() {
        // Spot-check the separable Gaussian against a hand computation.
        let mask = build_mask((11, 11), None);
        let win_w = 23;
        // center is exp(0)*exp(0) = 1
        approx::assert_abs_diff_eq!(mask[11 * win_w + 11], 1.0, epsilon = 1e-12);
        // corner (0,0): x=y=-1 => exp(-1)*exp(-1)
        approx::assert_abs_diff_eq!(mask[0], (-1.0f64).exp() * (-1.0f64).exp(), epsilon = 1e-12);
    }
}
