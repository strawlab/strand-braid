// Copyright 2022-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[derive(Debug)]
pub enum BoxData {
    String(String),
    // uses unicode string for storage as convenience. data are ASCII.
    AsciiString(String),
    DateTime(chrono::DateTime<chrono::Utc>),
    UnsignedInt(u32),
    Float(f32),
    Float64(f64),
    SimpleBlockData(BlockData),
    // uses unicode string for storage as convenience. data are [u8;4] ASCII.
    UncompressedFourCC(String),
}

/// A block of frame data. Typically corresponds to one frame.
#[derive(Debug)]
pub struct BlockData {
    /// the start position of the data.
    pub start: u64,
    /// the size of the data.
    pub size: u64,
    pub is_keyframe: bool,
    pub is_invisible: bool,
    pub is_discardable: bool,
    pub track_number: u64,
    pub timestamp: i16,
}

pub struct EbmlElement {
    /// the ID of the EBML element
    pub(crate) tag: Tag,
    /// the position of the start of the EBML element
    pub(crate) position: u64,
    /// the full size (header + data) of the EBML element
    pub(crate) full_size: u64,
    /// the data size of the EBML element
    pub(crate) data_size: u64,
    /// for master blocks, the children elements
    pub(crate) children: Vec<EbmlElement>,
    pub(crate) box_data: Option<BoxData>,
}

impl EbmlElement {
    #[inline]
    pub fn tag(&self) -> Tag {
        self.tag
    }
    #[inline]
    pub fn position(&self) -> u64 {
        self.position
    }
    #[inline]
    pub fn full_size(&self) -> u64 {
        self.full_size
    }
    #[inline]
    pub fn data_size(&self) -> u64 {
        self.data_size
    }
    #[inline]
    pub fn box_data(&self) -> Option<&BoxData> {
        self.box_data.as_ref()
    }
    #[inline]
    pub fn children(&self) -> &[EbmlElement] {
        &self.children
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Hex32(u32);

impl std::fmt::Debug for Hex32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:x}", self.0)
    }
}

macro_rules! impl_tags {
    ( $( ($name:ident, $val:expr, $dtype:expr) ),* ) => {
        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        pub enum Tag {
            $(
                $name,
            )*
            Other(Hex32),
        }

        impl Tag {
            pub fn dtype(&self) -> u8 {
                use Tag::*;
                match self {
                    $(
                        $name => $dtype,
                    )*
                    Other(_) => b'b',
                }
            }
        }

        impl From<u32> for Tag {
            fn from(id: u32) -> Tag {
                use Tag::*;
                match id {
                    $(
                        $val => $name,
                    )*
                    id => Other(Hex32(id)),
                }
            }
        }


    };
}

// See https://www.matroska.org/technical/elements.html
// https://github.com/ietf-wg-cellar/matroska-specification/blob/master/ebml_matroska.xml

impl_tags!(
    // These are not in Matroska spec (but in EBML spec?)
    (EBML, 0x1a45_dfa3, b'm'),
    (DocType, 0x4282, b'b'),
    (DocTypeReadVersion, 0x4285, b'b'),
    (Version, 0x4286, b'b'),
    (DocTypeVersion, 0x4287, b'b'),
    (ReadVersion, 0x42F7, b'b'),
    // These are in matroska spec
    (EBMLMaxIDLength, 0x42F2, b'u'),
    (EBMLMaxSizeLength, 0x42F3, b'u'),
    // Cues
    (Cues, 0x1c53_bb6b, b'm'),
    (CuePoint, 0xbb, b'b'),
    // Top-level singletons
    (Void, 0xEC, b'b'),
    (Info, 0x1549_A966, b'm'),
    // Segments
    (Segment, 0x1853_8067, b'm'),
    (SeekHead, 0x114D_9B74, b'b'),
    (Seek, 0x4DBB, b'b'),
    (SeekID, 0x53AB, b'b'),
    (SeekPosition, 0x53AC, b'b'),
    (TimestampScale, 0x2ad7b1, b'u'),
    (Duration, 0x4489, b'f'),
    (DateUTC, 0x4461, b'd'),
    (Title, 0x7BA9, b'8'),
    (MuxingApp, 0x4D80, b'8'),
    (WritingApp, 0x5741, b'8'),
    // Tracks
    (Tracks, 0x1654_ae6b, b'm'),
    (TrackEntry, 0xAE, b'm'),
    (TrackNumber, 0xD7, b'b'),
    (TrackUID, 0x73c5, b'b'),
    (TrackType, 0x83, b'b'),
    (CodecID, 0x86, b's'),
    (Video, 0xe0, b'm'),
    (PixelWidth, 0xB0, b'u'),
    (PixelHeight, 0xBA, b'u'),
    (UncompressedFourCC, 0x2eb524, b'b'),
    (GammaValue, 0x2fb523, b'f'),
    // Cluster
    (Cluster, 0x1F43_B675, b'm'),
    (Timestamp, 0xE7, b'u'),
    (SimpleBlock, 0xA3, b'b')
);
