//! Defines the WithKey trait for [Strand
//! Camera](https://strawlab.org/strand-cam) and
//! [Braid](https://strawlab.org/braid).

// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![warn(missing_docs)]

/// A trait for types that can provide a key of type `T`.
///
/// This trait is used throughout the Strand Camera ecosystem to provide a
/// consistent interface for objects that can be identified by a key. The key
/// type `T` is generic, allowing for different key types such as frame numbers,
/// timestamps, or custom identifiers.
///
/// # Type Parameters
///
/// * `T` - The type of the key that this object provides
///
/// # Examples
///
/// ```rust
/// use strand_withkey::WithKey;
///
/// // A simple struct that uses a string as its key
/// struct NamedItem {
///     name: String,
///     value: i32,
/// }
///
/// impl WithKey<String> for NamedItem {
///     fn key(&self) -> String {
///         self.name.clone()
///     }
/// }
///
/// let item = NamedItem {
///     name: "example".to_string(),
///     value: 100,
/// };
/// assert_eq!(item.key(), "example");
/// ```
///
/// ```rust
/// use strand_withkey::WithKey;
///
/// // A struct that uses a numeric ID as its key
/// struct NumberedFrame {
///     frame_id: u64,
///     timestamp: f64,
/// }
///
/// impl WithKey<u64> for NumberedFrame {
///     fn key(&self) -> u64 {
///         self.frame_id
///     }
/// }
///
/// let frame = NumberedFrame {
///     frame_id: 123,
///     timestamp: 1234567890.0,
/// };
/// assert_eq!(frame.key(), 123);
/// ```
pub trait WithKey<T> {
    /// Returns the key associated with this object.
    ///
    /// The key can be used to identify, index, or categorize this object
    /// within collections or processing pipelines.
    ///
    /// # Returns
    ///
    /// The key of type `T` associated with this object.
    fn key(&self) -> T;
}
