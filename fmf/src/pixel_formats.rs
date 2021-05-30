use crate::{FMFError, FMFResult};
use machine_vision_formats::PixFmt;

pub(crate) fn get_format(pixel_format: PixFmt) -> FMFResult<Vec<u8>> {
    use PixFmt::*;
    let r = match pixel_format {
        Mono8 => b"MONO8".to_vec(),
        BayerRG8 => b"RAW8:RGGB".to_vec(),
        BayerGB8 => b"RAW8:GBRG".to_vec(),
        BayerGR8 => b"RAW8:GRBG".to_vec(),
        BayerBG8 => b"RAW8:BGGR".to_vec(),
        YUV422 => b"YUV422".to_vec(),
        RGB8 => b"RGB8".to_vec(),
        other => {
            // So far we never saved Mono32f FMF formats and I am hesitant to
            // introduce it now.
            return Err(FMFError::UnimplementedPixelFormat(other));
        }
    };
    Ok(r)
}

pub(crate) fn get_pixel_format(format: &[u8]) -> FMFResult<PixFmt> {
    use PixFmt::*;
    match format {
        b"MONO8" => Ok(Mono8),
        b"RAW8:RGGB" | b"MONO8:RGGB" => Ok(BayerRG8),
        b"RAW8:GBRG" | b"MONO8:GBRG" => Ok(BayerGB8),
        b"RAW8:GRBG" | b"MONO8:GRBG" => Ok(BayerGR8),
        b"RAW8:BGGR" | b"MONO8:BGGR" => Ok(BayerBG8),
        b"YUV422" => Ok(YUV422),
        b"RGB8" => Ok(RGB8),
        f => Err(FMFError::UnknownFormat(
            String::from_utf8_lossy(&f).into_owned(),
        )),
    }
}
