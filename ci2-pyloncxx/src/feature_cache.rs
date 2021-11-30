use crate::Result;

/// cache for Pylon Feature Stream
pub(crate) struct PfsCache {
    headers: Vec<String>,
    /// All nodes, with preserved order.
    nodes: Vec<(String, String)>,
    /// Whether there is a single key per PFS
    strict: bool,
}

impl PfsCache {
    pub(crate) fn new_from_string(settings: String) -> Result<Self> {
        // I could not find any documentation about the PFS (Pylon Feature
        // System) format, so this is all a guess.
        let mut headers: Vec<String> = Default::default();
        let mut nodes: Vec<(String, String)> = Default::default();
        let mut header_done = false;
        let mut key_unique_check = std::collections::BTreeSet::new();
        for line in settings.lines() {
            let mut elements: Vec<String> = line.split('\t').map(Into::into).collect();
            if !header_done {
                if elements.len() == 1 || elements[0].starts_with('#') {
                    headers.push(line.to_string());
                    continue;
                } else {
                    header_done = true
                }
            }

            if elements.len() != 2 {
                panic!(
                    "expected PFS non-header line to have 2 tab-separated elements: {}",
                    line
                );
            }

            let mut d = elements.drain(..);
            let key = d.next().unwrap();
            let value = d.next().unwrap();
            key_unique_check.insert(key.clone());
            nodes.push((key, value));
        }

        let strict = key_unique_check.len() == nodes.len();

        Ok(Self {
            headers,
            nodes,
            strict,
        })
    }
    pub(crate) fn to_string(&self) -> String {
        // Again, I could not find any documentation about the PFS (Pylon
        // Feature System) format, so this is all a guess.
        let mut out_lines = self.headers.clone();
        for (key, value) in self.nodes.iter() {
            out_lines.push(format!("{}\t{}", key, value));
        }
        out_lines.join("\n")
    }
    pub(crate) fn update(&mut self, key: &str, value: String) {
        let mut found = false;
        for node in self.nodes.iter_mut() {
            if &node.0 == key {
                if found && self.strict {
                    panic!("Key \"{}\" exists more than once in PFS.", key);
                }
                found = true;
                node.1 = value.clone();
            }
        }
        if !found {
            log::warn!(
                "Attemped to store {}:{} to cache, but key not in cache.",
                key,
                value,
            );
        }
    }
}

pub(crate) trait PfsTrackedIntegerNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: i64) -> pylon_cxx::PylonResult<()>;
}

impl PfsTrackedIntegerNode for pylon_cxx::IntegerNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: i64) -> pylon_cxx::PylonResult<()> {
        self.set_value(new_value)?;
        pfs.update(self.name(), format!("{}", new_value));
        Ok(())
    }
}

pub(crate) trait PfsTrackedEnumNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: &str) -> pylon_cxx::PylonResult<()>;
}

impl PfsTrackedEnumNode for pylon_cxx::EnumNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: &str) -> pylon_cxx::PylonResult<()> {
        self.set_value(new_value)?;
        pfs.update(self.name(), format!("{}", new_value));
        Ok(())
    }
}

pub(crate) trait PfsTrackedFloatNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: f64) -> pylon_cxx::PylonResult<()>;
}

impl PfsTrackedFloatNode for pylon_cxx::FloatNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: f64) -> pylon_cxx::PylonResult<()> {
        self.set_value(new_value)?;
        pfs.update(self.name(), float_to_str(new_value));
        Ok(())
    }
}

/// Convert a float to a string with a minimum precision of 1.
///
/// This ensures at least a ".0" at the end to allow distinguishing this is a
/// float (from an int).
fn float_to_str(val: f64) -> String {
    let orig = format!("{}", val);
    if orig.contains('.') {
        orig
    } else {
        format!("{}.0", orig)
    }
}

#[test]
fn test_float_to_str() {
    assert_eq!(&float_to_str(0f64), "0.0");
    assert_eq!(&float_to_str(10.0), "10.0");
    assert_eq!(&float_to_str(10.000001), "10.000001");
}

pub(crate) trait PfsTrackedBooleanNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: bool) -> pylon_cxx::PylonResult<()>;
}

impl PfsTrackedBooleanNode for pylon_cxx::BooleanNode {
    fn set_value_pfs(&mut self, pfs: &mut PfsCache, new_value: bool) -> pylon_cxx::PylonResult<()> {
        self.set_value(new_value)?;
        pfs.update(self.name(), format!("{}", new_value as i8)); // make '0' or '1'
        Ok(())
    }
}
