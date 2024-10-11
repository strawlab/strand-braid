#![no_std]

/// Get all variants.
///
/// See also the IntoEnumIterator trait of the `strum` crate for a
/// version which can be automatically derived.
pub trait EnumIter
where
    Self: Sized,
{
    fn variants() -> &'static [Self];
}

impl EnumIter for bool {
    fn variants() -> &'static [Self] {
        &[true, false]
    }
}
