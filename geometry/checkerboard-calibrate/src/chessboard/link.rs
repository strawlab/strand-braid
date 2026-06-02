//! Linking quads into a board graph — stage 3 of `findChessboardCorners`.
//!
//! Adjacent chessboard squares touch at their corners, so the detected quads
//! form a graph: two quads are neighbors when one corner of each nearly
//! coincides. This mirrors OpenCV's `findQuadNeighbors` (corner matching) and
//! `findConnectedQuads` (grouping into connected components), which together
//! isolate the candidate board(s) from spurious quads.
//!
//! On a checkerboard the black squares meet only at corners and exactly two
//! squares meet at any shared corner, so each quad corner links to at most one
//! neighbor. Corner matching is therefore done as a global nearest-pair greedy
//! matching under a distance threshold proportional to the quads' edge lengths;
//! this is order-independent and yields the same board graph as OpenCV for a
//! clean board. (End-to-end parity with OpenCV is verified later against the
//! detection golden, since the intermediate graph is not exposed by OpenCV.)

use super::quad::Quad;

/// A quad participating in the board graph.
#[derive(Clone, Debug)]
pub struct LinkedQuad {
    /// Corner positions `(x, y)`. A linked pair of corners is snapped to their
    /// shared midpoint, as OpenCV merges coincident corners.
    pub corners: [(f32, f32); 4],
    /// For each corner, the index of the neighboring quad sharing it (if any).
    pub neighbors: [Option<usize>; 4],
    /// Minimum squared edge length of the quad (the neighbor-distance scale).
    pub edge_len: f32,
}

/// Fraction of the (squared) edge length within which two corners are treated
/// as the same shared corner. Shared corners of adjacent squares are
/// near-coincident, while the next-closest corners are about a full edge away
/// (squared distance ~= `edge_len`), so any value well below 1 separates them;
/// 0.25 (a linear gap up to half an edge) is generous but safe.
const LINK_THRESH_SCALE: f32 = 0.25;

fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

fn min_edge_len2(corners: &[(f32, f32); 4]) -> f32 {
    let mut m = f32::MAX;
    for i in 0..4 {
        m = m.min(dist2(corners[i], corners[(i + 1) % 4]));
    }
    m
}

/// Build [`LinkedQuad`]s from detected quads and link neighboring corners.
///
/// Two corners are eligible to link when their squared distance is within
/// [`LINK_THRESH_SCALE`] of the smaller of the two quads' edge-length scales.
/// Among all eligible corner pairs, the closest are matched first; each corner
/// links at most once.
pub fn link_quads(quads: &[Quad]) -> Vec<LinkedQuad> {
    let mut linked: Vec<LinkedQuad> = quads
        .iter()
        .map(|q| {
            let corners = [
                (q.corners[0].0 as f32, q.corners[0].1 as f32),
                (q.corners[1].0 as f32, q.corners[1].1 as f32),
                (q.corners[2].0 as f32, q.corners[2].1 as f32),
                (q.corners[3].0 as f32, q.corners[3].1 as f32),
            ];
            LinkedQuad {
                corners,
                neighbors: [None; 4],
                edge_len: min_edge_len2(&corners),
            }
        })
        .collect();

    // Collect all eligible corner pairs (i,ci)-(j,cj) with i < j.
    struct Cand {
        d: f32,
        i: usize,
        ci: usize,
        j: usize,
        cj: usize,
    }
    let mut cands = Vec::new();
    let n = linked.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let thr = linked[i].edge_len.min(linked[j].edge_len) * LINK_THRESH_SCALE;
            for ci in 0..4 {
                for cj in 0..4 {
                    let d = dist2(linked[i].corners[ci], linked[j].corners[cj]);
                    if d <= thr {
                        cands.push(Cand { d, i, ci, j, cj });
                    }
                }
            }
        }
    }
    cands.sort_by(|a, b| a.d.partial_cmp(&b.d).unwrap());

    for c in cands {
        if linked[c.i].neighbors[c.ci].is_none() && linked[c.j].neighbors[c.cj].is_none() {
            // Snap both corners to their shared midpoint.
            let a = linked[c.i].corners[c.ci];
            let b = linked[c.j].corners[c.cj];
            let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
            linked[c.i].corners[c.ci] = mid;
            linked[c.j].corners[c.cj] = mid;
            linked[c.i].neighbors[c.ci] = Some(c.j);
            linked[c.j].neighbors[c.cj] = Some(c.i);
        }
    }

    linked
}

/// Group quads into connected components following the neighbor links.
/// Returns each component as a list of quad indices.
pub fn connected_components(quads: &[LinkedQuad]) -> Vec<Vec<usize>> {
    let n = quads.len();
    let mut group = vec![usize::MAX; n];
    let mut groups = Vec::new();

    for start in 0..n {
        if group[start] != usize::MAX {
            continue;
        }
        let gid = groups.len();
        let mut members = Vec::new();
        let mut stack = vec![start];
        group[start] = gid;
        while let Some(q) = stack.pop() {
            members.push(q);
            for nb in quads[q].neighbors.into_iter().flatten() {
                if group[nb] == usize::MAX {
                    group[nb] = gid;
                    stack.push(nb);
                }
            }
        }
        members.sort_unstable();
        groups.push(members);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An axis-aligned square quad with given top-left corner and side.
    fn square(x: i32, y: i32, side: i32) -> Quad {
        Quad {
            corners: [(x, y), (x + side, y), (x + side, y + side), (x, y + side)],
        }
    }

    fn neighbor_count(q: &LinkedQuad) -> usize {
        q.neighbors.iter().filter(|n| n.is_some()).count()
    }

    #[test]
    fn two_quads_sharing_a_corner_link() {
        // Q1's top-left corner coincides with Q0's bottom-right corner.
        let quads = [square(0, 0, 20), square(20, 20, 20)];
        let linked = link_quads(&quads);
        assert_eq!(neighbor_count(&linked[0]), 1);
        assert_eq!(neighbor_count(&linked[1]), 1);
        let comps = connected_components(&linked);
        assert_eq!(comps, vec![vec![0, 1]]);
    }

    #[test]
    fn diagonal_chain_of_three() {
        let quads = [square(0, 0, 20), square(20, 20, 20), square(40, 40, 20)];
        let linked = link_quads(&quads);
        // The middle quad touches both ends.
        assert_eq!(neighbor_count(&linked[1]), 2);
        assert_eq!(neighbor_count(&linked[0]), 1);
        assert_eq!(neighbor_count(&linked[2]), 1);
        assert_eq!(connected_components(&linked), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn separated_quads_are_distinct_components() {
        let quads = [square(0, 0, 20), square(500, 500, 20)];
        let linked = link_quads(&quads);
        assert_eq!(neighbor_count(&linked[0]), 0);
        assert_eq!(neighbor_count(&linked[1]), 0);
        assert_eq!(connected_components(&linked), vec![vec![0], vec![1]]);
    }

    #[test]
    fn linked_corner_snaps_to_midpoint() {
        // Corners a pixel apart still link and snap to the midpoint.
        let q0 = square(0, 0, 20);
        let q1 = Quad {
            corners: [(21, 21), (41, 21), (41, 41), (21, 41)],
        };
        let linked = link_quads(&[q0, q1]);
        // Q0 corner 2 is (20,20); Q1 corner 0 is (21,21); midpoint (20.5,20.5).
        assert_eq!(linked[0].corners[2], (20.5, 20.5));
        assert_eq!(linked[1].corners[0], (20.5, 20.5));
    }
}
