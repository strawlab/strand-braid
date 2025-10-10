/// April Tags 2D detection coordinates.
///
/// Can be used for deserializing detections. In this case, other fields are
/// likely saved (e.g. `h00`), but currently we ignore those.
#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
pub struct AprilTagCoords2D {
    pub id: i32,
    pub hamming: i32,
    /// The h02 entry of the homography matrix.
    #[serde(rename = "h02")]
    pub x: f64,
    /// The h12 entry of the homography matrix.
    #[serde(rename = "h12")]
    pub y: f64,
}
