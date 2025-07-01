//! A small utility crate to provide [`EnumIter`] trait for iterating over enums
//! in the [Strand Camera](https://strawlab.org/strand-cam) ecosystem.

/// Allows collecting all variants of an enum.
///
/// See also the
/// [`IntoEnumIterator`](https://docs.rs/strum/0.27.1/strum/trait.IntoEnumIterator.html)
/// trait of the [`strum`](https://docs.rs/strum) crate for a version which can
/// be automatically derived.
pub trait EnumIter
where
    Self: Sized,
{
    /// Returns a vector containing all variants of the enum.
    fn variants() -> Vec<Self>;
}

impl EnumIter for bool {
    fn variants() -> Vec<Self> {
        vec![true, false]
    }
}
