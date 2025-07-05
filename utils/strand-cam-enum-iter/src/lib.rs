//! A small utility crate to provide [`EnumIter`] trait for iterating over enums
//! in the [Strand Camera](https://strawlab.org/strand-cam) ecosystem.

/// Allows collecting all variants of an enum.
///
/// See also the
/// [`IntoEnumIterator`](https://docs.rs/strum/0.27.1/strum/trait.IntoEnumIterator.html)
/// trait of the [`strum`](https://docs.rs/strum) crate for a version which can
/// be automatically derived.

// Copyright 2020-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

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
