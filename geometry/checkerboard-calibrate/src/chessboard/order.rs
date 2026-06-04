//! Ordering the board graph into a corner lattice — second half of stage 3.
//!
//! After [`super::link_quads`] builds the quad adjacency graph, the quads must
//! be laid out on the board's integer corner lattice so the inner corners can
//! be read off in a consistent row-major order. This mirrors the role of
//! OpenCV's `orderQuadCorners` / `orderFoundConnectedQuads`.
//!
//! Steps:
//!   1. [`order_all_corners`] puts each quad's four corners (and their neighbor
//!      links) into a consistent rotational order.
//!   2. [`assign_grid`] propagates integer lattice coordinates across a
//!      connected component by BFS over the neighbor links.
//!   3. [`ordered_inner_corners`] reads out the shared (inner) corners sorted
//!      row-major.
//!
//! Validated here with synthetic boards; end-to-end parity with OpenCV is
//! checked later against the detection golden.

use std::collections::HashMap;

use super::link::LinkedQuad;

/// Lattice offsets of a unit cell's four corners, in the same rotational order
/// produced by sorting corners by angle about their centroid (see
/// [`order_quad_corners`]). For the centroid-relative angle convention this is
/// the cyclic sequence top-left, top-right, bottom-right, bottom-left.
const CELL: [(i32, i32); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];

/// Reorder a quad's corners into a consistent rotational order (by angle about
/// the centroid), permuting the `neighbors` links the same way so each
/// `neighbors[i]` still refers to `corners[i]`.
pub fn order_quad_corners(quad: &mut LinkedQuad) {
    let cx = quad.corners.iter().map(|c| c.0).sum::<f32>() / 4.0;
    let cy = quad.corners.iter().map(|c| c.1).sum::<f32>() / 4.0;

    let mut idx = [0usize, 1, 2, 3];
    idx.sort_by(|&a, &b| {
        let aa = (quad.corners[a].1 - cy).atan2(quad.corners[a].0 - cx);
        let ab = (quad.corners[b].1 - cy).atan2(quad.corners[b].0 - cx);
        aa.partial_cmp(&ab).unwrap()
    });

    let corners = quad.corners;
    let neighbors = quad.neighbors;
    for (new_i, &old_i) in idx.iter().enumerate() {
        quad.corners[new_i] = corners[old_i];
        quad.neighbors[new_i] = neighbors[old_i];
    }
}

/// Order the corners of every quad in place.
pub fn order_all_corners(quads: &mut [LinkedQuad]) {
    for q in quads.iter_mut() {
        order_quad_corners(q);
    }
}

/// Integer lattice coordinate of each of a quad's four corners.
pub type QuadGrid = [(i32, i32); 4];

/// Propagate integer board-lattice coordinates across one connected component.
///
/// The seed quad is placed at the unit cell `[(0,0),(1,0),(1,1),(0,1)]`; each
/// neighbor is positioned so its shared corner matches the already-assigned
/// lattice coordinate, exploiting the fact that every quad shares the same
/// rotational corner order (so a quad's corners are always a translated unit
/// cell in lattice space). Returns the per-quad lattice coordinates keyed by
/// quad index.
///
/// Corners must already be ordered ([`order_all_corners`]).
pub fn assign_grid(quads: &[LinkedQuad], component: &[usize]) -> HashMap<usize, QuadGrid> {
    let mut coords: HashMap<usize, QuadGrid> = HashMap::new();
    if component.is_empty() {
        return coords;
    }

    let seed = component[0];
    coords.insert(seed, CELL);
    let mut stack = vec![seed];

    while let Some(a) = stack.pop() {
        let a_grid = coords[&a];
        for (ci, &shared) in a_grid.iter().enumerate() {
            let Some(b) = quads[a].neighbors[ci] else {
                continue;
            };
            if coords.contains_key(&b) {
                continue;
            }
            // The corner of `b` shared with `a`.
            let cj = quads[b]
                .neighbors
                .iter()
                .position(|&n| n == Some(a))
                .expect("neighbor link is reciprocal");

            // Origin so that b.corners[cj] lands on the shared lattice point.
            let ox = shared.0 - CELL[cj].0;
            let oy = shared.1 - CELL[cj].1;
            let b_grid: QuadGrid = std::array::from_fn(|k| (ox + CELL[k].0, oy + CELL[k].1));

            coords.insert(b, b_grid);
            stack.push(b);
        }
    }
    coords
}

/// Every lattice point referenced by at least one quad corner, mapped to its
/// averaged position and the number of referencing quad corners.
pub fn corner_lattice(
    quads: &[LinkedQuad],
    coords: &HashMap<usize, QuadGrid>,
) -> HashMap<(i32, i32), ((f32, f32), usize)> {
    // lattice point -> (summed position, count)
    let mut acc: HashMap<(i32, i32), ((f64, f64), usize)> = HashMap::new();
    for (&qi, grid) in coords {
        for (k, &cell) in grid.iter().enumerate() {
            let entry = acc.entry(cell).or_insert(((0.0, 0.0), 0));
            entry.0.0 += quads[qi].corners[k].0 as f64;
            entry.0.1 += quads[qi].corners[k].1 as f64;
            entry.1 += 1;
        }
    }

    acc.into_iter()
        .map(|(lattice, ((sx, sy), count))| {
            let n = count as f64;
            (lattice, (((sx / n) as f32, (sy / n) as f32), count))
        })
        .collect()
}

/// Inner board corners with their lattice coordinates (unordered).
///
/// Inner corners are the lattice points referenced by two or more quads (the
/// interior points where black squares meet); outer-boundary corners, touched
/// by a single quad, are excluded. Linked corners are already snapped to a
/// shared position, so all references to a lattice point coincide.
pub fn inner_corner_lattice(
    quads: &[LinkedQuad],
    coords: &HashMap<usize, QuadGrid>,
) -> Vec<((i32, i32), (f32, f32))> {
    corner_lattice(quads, coords)
        .into_iter()
        .filter(|(_, (_, count))| *count >= 2)
        .map(|(lattice, (pos, _))| (lattice, pos))
        .collect()
}

/// Read out the inner board corners from an assigned grid, sorted row-major
/// (by lattice row then column).
pub fn ordered_inner_corners(
    quads: &[LinkedQuad],
    coords: &HashMap<usize, QuadGrid>,
) -> Vec<(f32, f32)> {
    let mut inner = inner_corner_lattice(quads, coords);
    // Row-major: lattice y (row) then x (column).
    inner.sort_by_key(|a| (a.0.1, a.0.0));
    inner.into_iter().map(|(_, pt)| pt).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chessboard::link::{connected_components, link_quads};
    use crate::chessboard::quad::Quad;

    fn linked(corners: [(f32, f32); 4]) -> LinkedQuad {
        LinkedQuad {
            corners,
            neighbors: [None; 4],
            edge_len: 0.0,
        }
    }

    fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
        (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
    }

    #[test]
    fn orders_into_rotational_sequence() {
        let mut q = linked([(10.0, 10.0), (0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
        order_quad_corners(&mut q);
        for i in 0..4 {
            approx::assert_abs_diff_eq!(
                dist2(q.corners[i], q.corners[(i + 1) % 4]),
                100.0,
                epsilon = 1e-3
            );
        }
        approx::assert_abs_diff_eq!(dist2(q.corners[0], q.corners[2]), 200.0, epsilon = 1e-3);
    }

    #[test]
    fn permutes_neighbors_with_corners() {
        let mut q = linked([(10.0, 10.0), (0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
        q.neighbors[0] = Some(42);
        order_quad_corners(&mut q);
        let pos = q.corners.iter().position(|&c| c == (10.0, 10.0)).unwrap();
        assert_eq!(q.neighbors[pos], Some(42));
        assert_eq!(q.neighbors.iter().filter(|n| n.is_some()).count(), 1);
    }

    /// Build the black squares of a checkerboard with `cells` x `cells` cells,
    /// each `side` pixels, returning the quads. Black cells are those with
    /// `(cx + cy)` even.
    fn black_square_board(cells: i32, side: i32) -> Vec<Quad> {
        let mut quads = Vec::new();
        for cy in 0..cells {
            for cx in 0..cells {
                if (cx + cy) % 2 != 0 {
                    continue;
                }
                let x = cx * side;
                let y = cy * side;
                quads.push(Quad {
                    corners: [(x, y), (x + side, y), (x + side, y + side), (x, y + side)],
                });
            }
        }
        quads
    }

    #[test]
    fn recovers_inner_corner_lattice() {
        // 4x4 cells -> interior lattice points x,y in 1..=3 -> 3x3 = 9 inner corners.
        let side = 20;
        let quads = black_square_board(4, side);
        let mut linked = link_quads(&quads);
        order_all_corners(&mut linked);

        let comps = connected_components(&linked);
        assert_eq!(comps.len(), 1, "black squares should form one component");

        let grid = assign_grid(&linked, &comps[0]);
        let corners = ordered_inner_corners(&linked, &grid);

        // Expected: interior lattice points (x,y) in 1..=3, row-major, scaled.
        let mut expected = Vec::new();
        for y in 1..=3 {
            for x in 1..=3 {
                expected.push(((x * side) as f32, (y * side) as f32));
            }
        }
        assert_eq!(corners.len(), 9, "got {corners:?}");
        for (got, want) in corners.iter().zip(expected.iter()) {
            approx::assert_abs_diff_eq!(got.0, want.0, epsilon = 1e-3);
            approx::assert_abs_diff_eq!(got.1, want.1, epsilon = 1e-3);
        }
    }
}
