// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use bui_backend_session_types::ConnectionKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub theta: Option<f32>,
    pub area: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ToClient {
    pub fno: u64,
    pub firehose_frame_data_url: String,
    /// Indicates which region of the entire image is "valid".
    ///
    /// For example, when tracking, there can be image regions in which tracking
    /// is not desired. This is useful so the client can display what regions
    /// are valid.
    pub valid_display: Option<Shape>,
    /// Annotations associated with this particular image, e.g. from tracking.
    pub annotations: Vec<DrawableShape>,
    pub ts_rfc3339: String, // timestamp in RFC3339 format
    pub ck: ConnectionKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CircleParams {
    pub center_x: i16,
    pub center_y: i16,
    pub radius: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolygonParams {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Shape {
    Everything,
    Circle(CircleParams),
    // Hole(CircleParams),
    // Rectangle(RectangleParams),
    // Mask(MaskImage),
    Polygon(PolygonParams),
    /// multiple individual circles
    MultipleCircles(Vec<CircleParams>),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RgbaColor {
    r: u8,
    g: u8,
    b: u8,
    a: f32,
}

impl From<RgbaColor> for String {
    fn from(orig: RgbaColor) -> String {
        format!("rgba({}, {}, {}, {:.2})", orig.r, orig.g, orig.b, orig.a)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum StrokeStyle {
    CssColor(RgbaColor),
    // CanvasGradient,
    // CanvasPattern,
}

impl StrokeStyle {
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        StrokeStyle::CssColor(RgbaColor { r, g, b, a: 1.0 })
    }
}

impl From<StrokeStyle> for String {
    fn from(orig: StrokeStyle) -> String {
        match orig {
            StrokeStyle::CssColor(rgba) => rgba.into(),
        }
    }
}

/// A subset of the HTML5 canvas properties
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DrawableShape {
    shape: Shape,
    stroke_style: StrokeStyle,
    line_width: f32,
}

impl DrawableShape {
    pub fn from_shape(shape: &Shape, stroke_style: &StrokeStyle, line_width: f32) -> Self {
        Self {
            shape: shape.clone(),
            stroke_style: stroke_style.clone(),
            line_width,
        }
    }
}

/// internal type for using in javascript. convert from `DrawlableShape`.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct CanvasDrawableShape {
    pub shape: Shape,
    pub stroke_style: String,
    pub line_width: f32,
}

impl From<DrawableShape> for CanvasDrawableShape {
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

pub const VIDEO_STREAM_EVENT_NAME: &str = "http-video-streaming";

#[test]
fn test_polygon_from_yaml() {
    let mystr = "Polygon:
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
