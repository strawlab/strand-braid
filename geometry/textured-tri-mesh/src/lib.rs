#[cfg(feature = "serde-serialize")]
use serde::{Deserialize, Serialize};

/// A trimesh with 3D coords and texture coords.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-serialize", derive(Serialize, Deserialize))]
pub struct TriMesh {
    pub indices: Vec<[u32; 3]>,
    pub coords: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
}
