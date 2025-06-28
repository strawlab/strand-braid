//! Type definitions for HTTP video streaming functionality in the Strand Camera ecosystem.
//!
//! This crate provides serializable data structures for streaming video frames over HTTP
//! with annotations, shapes, and drawing capabilities. It's designed to work with
//! [Strand Camera](https://strawlab.org/strand-cam) and [Braid](https://strawlab.org/braid).
//!
//! ## Features
//!
//! - Video frame data structures with annotations
//! - Geometric shapes (circles, polygons) for region marking
//! - Drawing styles and colors for canvas rendering
//! - Serialization support via serde

// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use serde::{Deserialize, Serialize};
use strand_bui_backend_session_types::ConnectionKey;

/// Represents a 2D point with optional orientation and area information.
///
/// This structure describes detected features from tracking, where the point
/// may have additional properties like orientation and area.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Point {
    /// X coordinate of the point
    pub x: f32,
    /// Y coordinate of the point
    pub y: f32,
    /// Optional orientation angle in radians
    pub theta: Option<f32>,
    /// Optional area covered by the detected feature
    pub area: Option<f32>,
}

/// Message structure sent from server to client containing video frame data and annotations.
///
/// This structure encapsulates a complete video frame with all associated metadata,
/// annotations, and timing information for display in the client application.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ToClient {
    /// Frame number for ordering and synchronization
    pub fno: u64,
    /// Base64-encoded JPEG image data as a data URL
    pub firehose_frame_data_url: String,
    /// Indicates which region of the entire image is "valid".
    ///
    /// For example, when tracking, there can be image regions in which tracking
    /// is not desired. This is useful so the client can display what regions
    /// are valid.
    pub valid_display: Option<Shape>,
    /// Annotations associated with this particular image, e.g. from tracking.
    pub annotations: Vec<DrawableShape>,
    /// Timestamp in RFC3339 format when the frame was sent
    pub ts_rfc3339: String,
    /// Connection key identifying the client connection
    pub ck: ConnectionKey,
}

/// Parameters defining a circle shape.
///
/// Used for circular regions, annotations, or detected circular objects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CircleParams {
    /// X coordinate of the circle center
    pub center_x: i16,
    /// Y coordinate of the circle center
    pub center_y: i16,
    /// Radius of the circle
    pub radius: u16,
}

/// Parameters defining a polygon shape.
///
/// Used for arbitrary polygonal regions or complex shapes that cannot
/// be represented by simpler geometric primitives.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolygonParams {
    /// List of (x, y) coordinate pairs defining the polygon vertices
    pub points: Vec<(f64, f64)>,
}

// #[derive(Debug,Clone, Serialize, Deserialize, PartialEq)]
// pub struct RectangleParams {
//     pub lower_x: i16,
//     pub lower_y: i16,
//     pub width: u16,
//     pub height: u16,
// }

// #[derive(Debug,Clone, Serialize, Deserialize, PartialEq)]
// pub struct MaskImage {
//     pub width: u16,
//     pub height: u16,
//     pub data: Vec<u8>,
// }

/// Geometric shapes that can be used for regions, annotations, or masks.
///
/// This enum provides various geometric primitives for defining areas
/// of interest, valid regions, or annotation overlays on video frames.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Shape {
    /// Represents the entire image area
    Everything,
    /// A circular region
    Circle(CircleParams),
    // Hole(CircleParams),
    // Rectangle(RectangleParams),
    // Mask(MaskImage),
    /// A polygonal region with arbitrary vertices
    Polygon(PolygonParams),
    /// Multiple individual circles
    MultipleCircles(Vec<CircleParams>),
}

/// RGBA color representation for drawing operations.
///
/// Provides a standard color format with alpha transparency support
/// for use in drawing operations and style definitions.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RgbaColor {
    r: u8,
    g: u8,
    b: u8,
    a: f32,
}

impl From<RgbaColor> for String {
    /// Converts RGBA color to CSS color string format.
    ///
    /// # Returns
    ///
    /// A CSS-compatible color string in the format "rgba(r, g, b, a)".
    fn from(orig: RgbaColor) -> String {
        format!("rgba({}, {}, {}, {:.2})", orig.r, orig.g, orig.b, orig.a)
    }
}

/// Stroke style options for drawing shapes and annotations.
///
/// Currently supports RGBA colors.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum StrokeStyle {
    /// Solid color stroke using RGBA values
    CssColor(RgbaColor),
    // CanvasGradient,
    // CanvasPattern,
}

impl StrokeStyle {
    /// Creates a stroke style from RGB values with full opacity.
    ///
    /// # Arguments
    ///
    /// * `r` - Red component (0-255)
    /// * `g` - Green component (0-255)
    /// * `b` - Blue component (0-255)
    ///
    /// # Returns
    ///
    /// A new [`StrokeStyle`] with the specified RGB color and alpha = 1.0.
    ///
    /// # Example
    ///
    /// ```rust
    /// use strand_http_video_streaming_types::StrokeStyle;
    ///
    /// let green_stroke = StrokeStyle::from_rgb(0, 255, 0);
    /// ```
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        StrokeStyle::CssColor(RgbaColor { r, g, b, a: 1.0 })
    }
}

impl From<StrokeStyle> for String {
    /// Converts stroke style to CSS-compatible string representation.
    fn from(orig: StrokeStyle) -> String {
        match orig {
            StrokeStyle::CssColor(rgba) => rgba.into(),
        }
    }
}

/// A shape with associated drawing properties for canvas rendering.
///
/// Combines geometric shape information with visual styling properties to
/// create drawable annotations.
///
/// Typically this is constructed during the process of tracking and sent to the
/// client for rendering.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DrawableShape {
    /// The geometric shape to draw
    shape: Shape,
    /// Stroke style (color)
    stroke_style: StrokeStyle,
    /// Line width for drawing the shape outline
    line_width: f32,
}

impl DrawableShape {
    /// Creates a new drawable shape from a geometric shape and styling properties.
    pub fn from_shape(shape: &Shape, stroke_style: &StrokeStyle, line_width: f32) -> Self {
        Self {
            shape: shape.clone(),
            stroke_style: stroke_style.clone(),
            line_width,
        }
    }
}

/// Canvas-optimized representation of a drawable shape.
///
/// This structure is designed for efficient use in JavaScript/Canvas contexts
/// where stroke styles are represented as strings rather than structured data.
///
/// Typically this is constructed from a [DrawableShape] on the client
/// immediately before rendering.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct CanvasDrawableShape {
    /// The geometric shape to draw
    pub shape: Shape,
    /// Stroke style as a CSS-compatible string
    pub stroke_style: String,
    /// Line width for drawing the shape outline
    pub line_width: f32,
}

impl From<DrawableShape> for CanvasDrawableShape {
    /// Converts a DrawableShape to a CanvasDrawableShape.
    ///
    /// This conversion prepares the shape for use in canvas rendering contexts
    /// by converting the structured stroke style to a CSS string.
    fn from(orig: DrawableShape) -> CanvasDrawableShape {
        CanvasDrawableShape {
            shape: orig.shape,
            stroke_style: orig.stroke_style.into(),
            line_width: orig.line_width,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_convert_drawable_shape() {
        let cps = CircleParams {
            center_x: 100,
            center_y: 200,
            radius: 50,
        };
        let shape = Shape::Circle(cps);
        let ss = StrokeStyle::from_rgb(1, 2, 3);
        let ds = DrawableShape::from_shape(&shape, &ss, 1.0);
        let cds: CanvasDrawableShape = ds.into();
        assert_eq!(cds.stroke_style, "rgba(1, 2, 3, 1.00)");
    }
}

/// Event name used for Server-Sent Events (SSE) video streaming.
///
/// This constant defines the event type identifier used in the SSE protocol
/// for transmitting video frame data and annotations to connected clients.
pub const VIDEO_STREAM_EVENT_NAME: &str = "http-video-streaming";

#[test]
fn test_polygon_from_yaml() {
    let mystr = "!Polygon
    points:
      - [510.0, 520.0]
      - [520.0, 530.0]
      - [510.0, 540.0]
";
    let polygon = Shape::Polygon(PolygonParams {
        points: vec![(510.0, 520.0), (520.0, 530.0), (510.0, 540.0)],
    });

    let polygon2: Shape = serde_yaml::from_str(&mystr).unwrap();
    assert_eq!(polygon, polygon2);
}

#[test]
fn test_multiple_circles_yaml_roundtrip() {
    let circles = Shape::MultipleCircles(vec![
        CircleParams {
            center_x: 1,
            center_y: 2,
            radius: 34,
        },
        CircleParams {
            center_x: 10,
            center_y: 20,
            radius: 345,
        },
        CircleParams {
            center_x: 100,
            center_y: 200,
            radius: 340,
        },
    ]);

    let mystr = serde_yaml::to_string(&circles).unwrap();
    dbg!(&mystr);
    let circles2: Shape = serde_yaml::from_str(&mystr).unwrap();
    assert_eq!(circles, circles2);
}
