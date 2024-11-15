/// Get all variants.
///
/// See also the IntoEnumIterator trait of the `strum` crate for a
/// version which can be automatically derived.
pub trait EnumIter
where
    Self: Sized,
{
    fn variants() -> Vec<Self>;
}

impl EnumIter for bool {
    fn variants() -> Vec<Self> {
        vec![true, false]
    }
}
