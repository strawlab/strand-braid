// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure-Rust port of OpenCV's `cv::findChessboardCorners` (the classic
//! contour-based detector, not `findChessboardCornersSB`).
//!
//! This is the largest of the OpenCV routines being replaced. It is built
//! bottom-up:
//!   1. binarization primitives ([`binarize`]) — histogram equalization
//!      (`CALIB_CB_NORMALIZE_IMAGE`) and adaptive-mean thresholding
//!      (`CALIB_CB_ADAPTIVE_THRESH`) — **done**, bit-exact vs OpenCV,
//!   2. quad generation from contours — **done**, all cross-checked vs OpenCV:
//!      contour finding ([`find_contours`], Suzuki-Abe), polygon approximation
//!      ([`approx_poly_dp`], Douglas-Peucker), and the convex-4-gon area filter
//!      ([`find_quads`], with [`contour_area`] / [`is_contour_convex`]),
//!   3. linking quads into a board graph and ordering corners — **done**
//!      (synthetic tests): neighbor linking ([`link_quads`]), connected
//!      components ([`connected_components`]), consistent corner ordering
//!      ([`order_all_corners`]), board-lattice propagation ([`assign_grid`]),
//!      and row-major inner-corner readout ([`ordered_inner_corners`]),
//!   4. board validation (size, monotonicity) and corner extraction — **done**
//!      (synthetic tests): [`check_board_monotony`] and [`extract_board`].
//!
//! The stages are wired together in [`find_chessboard_corners`], which on the
//! OpenCV `left*.jpg` samples recovers OpenCV's corner positions to <=0.17px on
//! all 13 frames. Incomplete boards
//! are filled at the lattice level (see [`extract_board`]). The only behavior
//! not replicated is OpenCV's exact output corner order, which is pose-dependent
//! and not required for the pure-Rust calibrator.
//!
//! The detector flags requested by the strand-braid C++ wrapper are
//! `CALIB_CB_ADAPTIVE_THRESH | CALIB_CB_NORMALIZE_IMAGE | CALIB_CB_FAST_CHECK`.

mod approx;
mod binarize;
mod board;
mod contour;
mod detect;
mod link;
mod order;
mod quad;

pub use approx::approx_poly_dp;
pub use binarize::{adaptive_threshold_mean, equalize_hist};
pub use board::{check_board_monotony, extract_board};
pub use contour::{Contour, find_contours};
pub use detect::find_chessboard_corners;
pub use link::{LinkedQuad, connected_components, link_quads};
pub use order::{
    QuadGrid, assign_grid, corner_lattice, inner_corner_lattice, order_all_corners,
    order_quad_corners, ordered_inner_corners,
};
pub use quad::{Quad, contour_area, find_quads, is_contour_convex};
