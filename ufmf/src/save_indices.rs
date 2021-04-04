use crate::*;
use byteorder::{LittleEndian, WriteBytesExt};

pub(crate) fn save_indices<F: Write + Seek>(
    f: &mut F,
    index_frame: &Vec<TimestampLoc>,
    index_keyframes: &BTreeMap<Vec<u8>, Vec<TimestampLoc>>,
) -> UFMFResult<usize> {
    let mut pos = 0;
    pos += start_dict(f, 2)?;

    // write frame dict
    pos += write_key(f, b"frame")?;
    pos += write_idx(f, index_frame)?;

    pos += write_key(f, b"keyframe")?;
    {
        let n_keyframe_types = cast::u8(index_keyframes.len())?;
        pos += start_dict(f, n_keyframe_types)?;
        for (keyframe_type, keyframe_index) in index_keyframes.iter() {
            pos += write_key(f, keyframe_type)?;
            pos += write_idx(f, keyframe_index)?;
        }
    }

    Ok(pos)
}

fn write_idx<F: Write + Seek>(f: &mut F, idx: &Vec<TimestampLoc>) -> UFMFResult<usize> {
    let mut pos = 0;

    if idx.len() > 0 {
        let locs = idx.iter().map(|x| x.loc).collect();
        let timestamps = idx.iter().map(|x| x.timestamp).collect();

        pos += start_dict(f, 2)?;
        pos += write_key(f, b"loc")?;
        pos += write_locs(f, &locs)?;
        pos += write_key(f, b"timestamp")?;
        pos += write_timestamps(f, &timestamps)?;
    } else {
        pos += start_dict(f, 0)?;
    }
    Ok(pos)
}

fn start_dict<F: Write + Seek>(f: &mut F, n_keys: u8) -> UFMFResult<usize> {
    let mut pos = 0;
    pos += f.write(&[b'd', n_keys])?;
    Ok(pos)
}

fn write_key<F: Write + Seek>(f: &mut F, key: &[u8]) -> UFMFResult<usize> {
    let mut pos = 0;
    let buf0 = structure!("<H").pack(cast::u16(key.len())?)?;
    let buf1 = key;
    pos += f.write(&buf0)?;
    pos += f.write(&buf1)?;
    Ok(pos)
}

fn write_locs<F: Write + Seek>(f: &mut F, locs: &Vec<u64>) -> UFMFResult<usize> {
    let mut pos = 0;
    let dtype_char = b'l';
    let bytes_per_element = 8;

    pos += f.write(&[b'a', dtype_char])?;

    let buf = structure!("<I").pack(cast::u32(locs.len() * bytes_per_element)?)?;
    pos += f.write(&buf)?;

    for loc in locs {
        f.write_u64::<LittleEndian>(*loc)?;
        pos += 8;
    }

    Ok(pos)
}

fn write_timestamps<F: Write + Seek>(f: &mut F, timestamps: &Vec<f64>) -> UFMFResult<usize> {
    let mut pos = 0;
    let dtype_char = b'd';
    let bytes_per_element = 8;

    pos += f.write(&[b'a', dtype_char])?;

    let buf = structure!("<I").pack(cast::u32(timestamps.len() * bytes_per_element)?)?;
    pos += f.write(&buf)?;

    for timestamp in timestamps {
        f.write_f64::<LittleEndian>(*timestamp)?;
        pos += 8;
    }

    Ok(pos)
}
