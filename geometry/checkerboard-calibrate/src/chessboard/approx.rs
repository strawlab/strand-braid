//! Polygon approximation — a port of OpenCV's `approxPolyDP` (the Douglas–Peucker
//! variant in `modules/imgproc/src/approx.cpp`).
//!
//! OpenCV's implementation is a specific stack-based Douglas–Peucker followed by
//! a collinear-point cleanup pass, with particular wraparound/rounding behavior.
//! This is ported literally so the output vertices match OpenCV exactly. The chessboard
//! detector only ever calls it with `closed = true`, but the open-contour path
//! is ported too for completeness.

/// Approximate a polygonal curve with the Douglas–Peucker algorithm.
///
/// `eps` is the maximum distance (in pixels) between the original curve and its
/// approximation. Returns the approximated vertices in order.
pub fn approx_poly_dp(src: &[(i32, i32)], eps: f64, closed: bool) -> Vec<(i32, i32)> {
    let count = src.len();
    if count == 0 {
        return Vec::new();
    }
    let eps = eps * eps;

    let mut dst: Vec<(i32, i32)> = Vec::with_capacity(count);
    let mut stack: Vec<(usize, usize)> = Vec::new();

    let mut slice = (0usize, 0usize);
    let mut right = (0usize, 0usize);
    let mut pos = 0usize;
    let mut start_pt = (0i32, 0i32);

    // An "open" contour whose endpoints coincide is treated as closed, matching
    // OpenCV.
    let is_closed = closed || src[0] == src[count - 1];

    if !is_closed {
        stack.push((0, count - 1));
    } else {
        right.0 = 0;
        let mut le_eps = false;
        for _ in 0..3 {
            let mut max_dist = 0.0f64;
            pos = (pos + right.0) % count;
            start_pt = src[pos];
            pos = (pos + 1) % count;
            for j in 1..count {
                let pt = src[pos];
                pos = (pos + 1) % count;
                let dx = (pt.0 - start_pt.0) as f64;
                let dy = (pt.1 - start_pt.1) as f64;
                let dist = dx * dx + dy * dy;
                if dist > max_dist {
                    max_dist = dist;
                    right.0 = j;
                }
            }
            le_eps = max_dist <= eps;
        }

        if !le_eps {
            let tmp = pos % count;
            right.1 = tmp;
            slice.0 = tmp;
            let newv = (right.0 + slice.0) % count;
            slice.1 = newv;
            right.0 = newv;
            stack.push(right);
            stack.push(slice);
        } else {
            // Whole contour fits within eps: after the init loop `pos` is back
            // at the position where `start_pt` was read.
            dst.push(start_pt);
        }
    }

    // Recursive subdivision (iterative via the stack).
    while let Some(s) = stack.pop() {
        slice = s;
        let end_pt = src[slice.1];
        pos = slice.0;
        start_pt = src[pos];
        pos = (pos + 1) % count;

        let le_eps = if pos != slice.1 {
            let dx = (end_pt.0 - start_pt.0) as f64;
            let dy = (end_pt.1 - start_pt.1) as f64;
            let mut max_dist = 0.0f64;
            while pos != slice.1 {
                let pt = src[pos];
                pos = (pos + 1) % count;
                let dist =
                    ((pt.1 - start_pt.1) as f64 * dx - (pt.0 - start_pt.0) as f64 * dy).abs();
                if dist > max_dist {
                    max_dist = dist;
                    right.0 = (pos + count - 1) % count;
                }
            }
            max_dist * max_dist <= eps * (dx * dx + dy * dy)
        } else {
            true
        };

        if le_eps {
            dst.push(start_pt);
        } else {
            right.1 = slice.1;
            slice.1 = right.0;
            stack.push(right);
            stack.push(slice);
        }
    }

    if !is_closed {
        dst.push(src[count - 1]);
    }

    cleanup_collinear(&mut dst, eps, is_closed);
    dst
}

/// Final stage of OpenCV `approxPolyDP`: drop points lying on (almost) straight
/// lines between their neighbors. Operates in place; `dst` is truncated to the
/// surviving vertices.
fn cleanup_collinear(dst: &mut Vec<(i32, i32)>, eps: f64, closed: bool) {
    let count = dst.len();
    if count < 3 {
        return;
    }
    let mut new_count = count;

    let mut pos = if closed { count - 1 } else { 0 };
    let mut start_pt = dst[pos];
    pos = (pos + 1) % count;
    let mut wpos = pos;
    let mut pt = dst[pos];
    pos = (pos + 1) % count;

    let lo = if closed { 0 } else { 1 };
    let hi = count - if closed { 0 } else { 1 };
    let mut i = lo;
    while i < hi && new_count > 2 {
        let end_pt = dst[pos];
        pos = (pos + 1) % count;

        let dx = (end_pt.0 - start_pt.0) as f64;
        let dy = (end_pt.1 - start_pt.1) as f64;
        let dist = ((pt.0 - start_pt.0) as f64 * dy - (pt.1 - start_pt.1) as f64 * dx).abs();
        let sip = (pt.0 - start_pt.0) as f64 * (end_pt.0 - pt.0) as f64
            + (pt.1 - start_pt.1) as f64 * (end_pt.1 - pt.1) as f64;

        if dist * dist <= 0.5 * eps * (dx * dx + dy * dy) && dx != 0.0 && dy != 0.0 && sip >= 0.0 {
            new_count -= 1;
            start_pt = end_pt;
            dst[wpos] = end_pt;
            wpos = (wpos + 1) % count;
            pt = dst[pos];
            pos = (pos + 1) % count;
            i += 2;
            continue;
        }

        start_pt = pt;
        dst[wpos] = pt;
        wpos = (wpos + 1) % count;
        pt = end_pt;
        i += 1;
    }

    if !closed {
        dst[wpos] = pt;
    }
    dst.truncate(new_count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_dense_edges_reduces_to_four_corners() {
        // A 100x100 square sampled densely along each edge (closed contour).
        let mut pts = Vec::new();
        for x in 0..=100 {
            pts.push((x, 0));
        }
        for y in 1..=100 {
            pts.push((100, y));
        }
        for x in (0..100).rev() {
            pts.push((x, 100));
        }
        for y in (1..100).rev() {
            pts.push((0, y));
        }

        let approx = approx_poly_dp(&pts, 3.0, true);
        assert_eq!(approx.len(), 4, "expected 4 corners, got {approx:?}");
        let set: std::collections::BTreeSet<_> = approx.iter().copied().collect();
        for corner in [(0, 0), (100, 0), (100, 100), (0, 100)] {
            assert!(
                set.contains(&corner),
                "missing corner {corner:?} in {approx:?}"
            );
        }
    }

    #[test]
    fn collinear_points_collapse() {
        // A straight, closed back-and-forth degenerate shape reduces heavily.
        let pts = vec![(0, 0), (10, 0), (20, 0), (30, 0), (30, 10), (0, 10)];
        let approx = approx_poly_dp(&pts, 1.0, true);
        // The three collinear top points (10,0)/(20,0) collapse; corners remain.
        assert!(approx.len() <= 4, "got {approx:?}");
        assert!(approx.contains(&(0, 0)));
    }
}
