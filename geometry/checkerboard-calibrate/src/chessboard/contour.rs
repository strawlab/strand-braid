//! Suzuki–Abe border following — a port of OpenCV's `findContours`
//! (`RETR_LIST`, `CHAIN_APPROX_NONE`).
//!
//! Reference: S. Suzuki and K. Abe, "Topological structural analysis of
//! digitized binary images by border following", CVGIP 30(1), 1985. OpenCV's
//! `findContours` implements the same algorithm, so the set of border pixels
//! produced here matches OpenCV exactly. Ordering within a contour is an
//! implementation detail and is not required to match.
//!
//! Input is a binary image (0 = background, non-zero = foreground). Output is
//! the list of borders; each border is a closed sequence of `(x, y)` pixels,
//! flagged as an outer or hole border.

/// A traced border.
#[derive(Clone, Debug)]
pub struct Contour {
    /// Border pixels as `(x, y)` = `(col, row)`, in trace order.
    pub points: Vec<(i32, i32)>,
    /// Whether this is a hole border (background enclosed by foreground) as
    /// opposed to an outer border.
    pub is_hole: bool,
}

/// Clockwise 8-neighborhood offsets `(d_row, d_col)`, index 0 = East.
const NEI: [(i32, i32); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

fn dir_index(dr: i32, dc: i32) -> usize {
    NEI.iter().position(|&(r, c)| r == dr && c == dc).unwrap()
}

#[inline]
fn get(img: &[i32], w: i32, h: i32, r: i32, c: i32) -> i32 {
    if r < 0 || c < 0 || r >= h || c >= w {
        0
    } else {
        img[(r * w + c) as usize]
    }
}

/// Find all borders in a binary image.
pub fn find_contours(bin: &[u8], width: usize, height: usize) -> Vec<Contour> {
    assert_eq!(bin.len(), width * height, "bin length must be width*height");
    let (w, h) = (width as i32, height as i32);
    let mut img: Vec<i32> = bin.iter().map(|&v| (v != 0) as i32).collect();

    let mut contours = Vec::new();
    // Border counter; 1 is the implicit image frame. The full Suzuki-Abe
    // hierarchy (parent links via the last-seen border number) is not
    // reconstructed here — the quad detector only needs the borders and their
    // outer/hole type.
    let mut nbd = 1i32;

    for i in 0..h {
        for j in 0..w {
            let fij = img[(i * w + j) as usize];
            if fij == 0 {
                continue;
            }

            let from;
            let is_hole;
            if fij == 1 && get(&img, w, h, i, j - 1) == 0 {
                // Outer border start: foreground pixel with background to the left.
                nbd += 1;
                from = (i, j - 1);
                is_hole = false;
            } else if fij >= 1 && get(&img, w, h, i, j + 1) == 0 {
                // Hole border start: pixel with background to the right.
                nbd += 1;
                from = (i, j + 1);
                is_hole = true;
            } else {
                continue;
            }

            let mut points = Vec::new();
            border_follow(&mut img, w, h, (i, j), from, nbd, &mut points);
            contours.push(Contour { points, is_hole });
        }
    }
    contours
}

/// Trace one border starting at `start`, coming from background pixel `from`,
/// labeling visited pixels with `nbd`.
fn border_follow(
    img: &mut [i32],
    w: i32,
    h: i32,
    start: (i32, i32),
    from: (i32, i32),
    nbd: i32,
    points: &mut Vec<(i32, i32)>,
) {
    let (i, j) = start;
    let idx = |r: i32, c: i32| (r * w + c) as usize;

    // 3.1: clockwise from `from`, find the first foreground neighbor.
    let from_dir = dir_index(from.0 - i, from.1 - j);
    let mut found = None;
    for k in 0..8 {
        let d = (from_dir + k) & 7;
        let (r, c) = (i + NEI[d].0, j + NEI[d].1);
        if get(img, w, h, r, c) != 0 {
            found = Some((r, c));
            break;
        }
    }
    let (i1, j1) = match found {
        // Isolated pixel: a single-point border.
        None => {
            img[idx(i, j)] = -nbd;
            points.push((j, i));
            return;
        }
        Some(p) => p,
    };

    let (mut i2, mut j2) = (i1, j1);
    let (mut i3, mut j3) = (i, j);
    loop {
        // 3.3: counterclockwise around (i3,j3) starting one step CCW from (i2,j2).
        let d2 = dir_index(i2 - i3, j2 - j3);
        let mut examined_east_zero = false;
        let mut p4 = None;
        for k in 1..=8 {
            let d = (d2 + 8 - k) & 7;
            let (r, c) = (i3 + NEI[d].0, j3 + NEI[d].1);
            let val = get(img, w, h, r, c);
            if d == 0 && val == 0 {
                // The east neighbor was examined and is background.
                examined_east_zero = true;
            }
            if val != 0 {
                p4 = Some((r, c));
                break;
            }
        }
        let (i4, j4) = p4.expect("border closes on itself");

        // 3.4: label the current pixel.
        if examined_east_zero {
            img[idx(i3, j3)] = -nbd;
        } else if img[idx(i3, j3)] == 1 {
            img[idx(i3, j3)] = nbd;
        }
        points.push((j3, i3));

        // 3.5: stop when we return to the start along the first edge.
        if (i4, j4) == (i, j) && (i3, j3) == (i1, j1) {
            break;
        }
        (i2, j2) = (i3, j3);
        (i3, j3) = (i4, j4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn img(rows: &[&str]) -> (Vec<u8>, usize, usize) {
        let h = rows.len();
        let w = rows[0].len();
        let mut data = vec![0u8; w * h];
        for (r, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                data[r * w + c] = if ch == '#' { 255 } else { 0 };
            }
        }
        (data, w, h)
    }

    fn pointset(c: &Contour) -> BTreeSet<(i32, i32)> {
        c.points.iter().copied().collect()
    }

    #[test]
    fn single_pixel() {
        let (data, w, h) = img(&[".....", ".....", "..#..", ".....", "....."]);
        let cs = find_contours(&data, w, h);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].points, vec![(2, 2)]);
        assert!(!cs[0].is_hole);
    }

    #[test]
    fn solid_square_outer_border() {
        let (data, w, h) = img(&[".....", ".###.", ".###.", ".###.", "....."]);
        let cs = find_contours(&data, w, h);
        assert_eq!(cs.len(), 1);
        assert!(!cs[0].is_hole);
        // The border is the 8 perimeter pixels (the center is interior).
        let expected: BTreeSet<(i32, i32)> = [
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (3, 2),
            (1, 3),
            (2, 3),
            (3, 3),
        ]
        .into_iter()
        .collect();
        assert_eq!(pointset(&cs[0]), expected);
    }

    #[test]
    fn ring_has_outer_and_hole_borders() {
        // 5x5 solid block with a single-pixel hole in the middle.
        let (data, w, h) = img(&[
            ".......", ".#####.", ".#####.", ".##.##.", ".#####.", ".#####.", ".......",
        ]);
        let cs = find_contours(&data, w, h);
        assert_eq!(cs.len(), 2, "expected one outer and one hole border");
        assert!(!cs[0].is_hole, "first found border should be outer");
        assert!(cs[1].is_hole, "second should be the hole border");
        // Outer border = the 16 perimeter pixels of the 5x5 block.
        assert_eq!(
            cs[0].points.iter().copied().collect::<BTreeSet<_>>().len(),
            16
        );
        // Hole border surrounds the missing pixel at (3,3): its 8 neighbors.
        let hole = pointset(&cs[1]);
        assert!(hole.contains(&(2, 3)) && hole.contains(&(4, 3)));
        assert!(!hole.contains(&(3, 3)));
    }

    #[test]
    fn two_separate_squares() {
        let (data, w, h) = img(&[".......", ".##.##.", ".##.##.", "......."]);
        let cs = find_contours(&data, w, h);
        assert_eq!(cs.len(), 2);
        assert!(cs.iter().all(|c| !c.is_hole));
    }
}
