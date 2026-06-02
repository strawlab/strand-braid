//! Pure-Rust reimplementations of the OpenCV calibration routines used by
//! strand-braid. See the crate README for scope and validation strategy.

pub mod calibrate;
pub mod chessboard;
mod corner_subpix;

pub use corner_subpix::{CornerSubPixParams, GrayImageRef, corner_subpix};
