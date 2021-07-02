#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct VirtualDisplayName(pub String);

/// A physical display (projector or monitor),
///
/// A single `Display` has one or more `VirtualDisplays`.
///
/// When calibrating with a pinhole model, a `Display` theoretically has a
/// single set of intrinsic parameters across all virtual displays. However,
/// this theoretical consideration might not be followed. Each virtual display
/// has an independent set of extrinsic parameters.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(non_snake_case)]
pub struct Display {
    pub width: usize,
    pub height: usize,
    #[serde(rename = "virtualDisplays")]
    pub virtual_displays: Vec<VirtualDisplay>,
}

/// A SimpleDisplay has a single VirtualDisplay
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(non_snake_case)]
pub struct SimpleDisplay {
    pub width: usize,
    pub height: usize,
}

impl SimpleDisplay {
    pub(crate) fn to_orig(self, name: &str) -> Display {
        let vdisp = VirtualDisplay {
            id: VirtualDisplayName(name.to_string()),
            viewport: vec![
                (0, 0),
                (self.width, 0),
                (self.width, self.height),
                (0, self.height),
            ],
            mirror: Mirror::None,
        };
        Display {
            width: self.width,
            height: self.height,
            virtual_displays: vec![vdisp],
        }
    }
}

/// A VirtualDisplay shares the same intrinsic parameters with its parent
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VirtualDisplay {
    pub id: VirtualDisplayName,
    pub viewport: Vec<(usize, usize)>,
    #[serde(default)]
    pub mirror: Mirror,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Mirror {
    None,
    #[serde(rename = "lr")]
    Lr,
}
impl std::default::Default for Mirror {
    fn default() -> Mirror {
        Mirror::None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
/// Used to determine a mapping between display and model points
pub struct CompleteCorrespondance {
    pub triangle_index: usize,
    pub triangle_vertex_index: usize,
    pub display_x: f64,
    pub display_y: f64,
    pub display_depth: f64,
    pub texture_u: f64,
    pub texture_v: f64,
    pub vertex_x: f64,
    pub vertex_y: f64,
    pub vertex_z: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
/// Used to determine a mapping between display and model points
pub struct SimpleUVCorrespondance {
    pub display_x: f64,
    pub display_y: f64,
    pub texture_u: f64,
    pub texture_v: f64,
}

impl SimpleUVCorrespondance {
    pub(crate) fn to_orig(self, name: &str) -> UVCorrespondance {
        UVCorrespondance {
            virtual_display: VirtualDisplayName(name.to_string()),
            display_x: self.display_x,
            display_y: self.display_y,
            texture_u: self.texture_u,
            texture_v: self.texture_v,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
/// Used to determine a mapping between display and model points
pub struct UVCorrespondance {
    pub display_x: f64,
    pub display_y: f64,
    pub texture_u: f64,
    pub texture_v: f64,
    pub virtual_display: VirtualDisplayName,
}

pub type VDispInfo = (ncollide_geom::Mask, Vec<f64>, usize);
