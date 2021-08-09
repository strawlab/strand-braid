use crate::{serialport, NameType, DEVICE_NAME_LEN};
use anyhow::Result;

fn reset_device(device: &mut Box<dyn serialport::SerialPort>) -> Result<()> {
    device.write_request_to_send(false)?;
    device.write_data_terminal_ready(false)?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    device.write_request_to_send(true)?;
    device.write_data_terminal_ready(true)?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    Ok(())
}

fn flush_device<W: std::io::Write>(ser: &mut W) -> Result<()> {
    for _ in 0..5 {
        ser.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

fn _crc8maxim(mut crc: u8, c: &u8) -> u8 {
    crc ^= c;
    for _ in 0..8 {
        if (crc & 0x01) > 0 {
            crc = (crc >> 1) ^ 0x8C;
        } else {
            crc = crc >> 1;
        }
    }
    crc
}

pub(crate) fn crc8maxim(s: &[u8]) -> u8 {
    let mut crc = 0;
    for c in s {
        crc = _crc8maxim(crc, c);
    }
    crc
}

#[derive(Debug, thiserror::Error)]
enum UdevError {
    #[error("CRC failed")]
    CrcFail,
    #[error("IO error {0}")]
    Io(#[from] std::io::Error),
}

fn get_device_name(
    device: &mut Box<dyn serialport::SerialPort>,
) -> std::result::Result<NameType, UdevError> {
    use std::io::{Read, Write};
    device.write(b"N?")?;

    let alloc_len = DEVICE_NAME_LEN + 2;

    let mut buf = vec![0; alloc_len];

    // Wait half second for full answer.
    std::thread::sleep(std::time::Duration::from_millis(500));

    let len = device.read(&mut buf)?;
    assert!(len > DEVICE_NAME_LEN, "No CRC returned");

    let name_and_crc = &buf[..len];
    trace!("get_device_name read {} bytes: {:?}", len, name_and_crc);
    let name = &name_and_crc[..DEVICE_NAME_LEN];
    let crc_buf = &name_and_crc[DEVICE_NAME_LEN..];
    let expected_crc = std::str::from_utf8(crc_buf).expect("from utf8");
    trace!("expected CRC: {:?}", expected_crc);

    let computed_crc = format!("{:X}", crc8maxim(name));
    trace!("computed CRC: {:?}", computed_crc);
    if &computed_crc == expected_crc {
        let mut result = [0; DEVICE_NAME_LEN];
        result[..DEVICE_NAME_LEN].copy_from_slice(&name);
        Ok(Some(result))
    } else {
        Err(UdevError::CrcFail)
    }
}

pub fn serial_handshake(
    port: &std::path::Path,
) -> Result<(Box<dyn serialport::SerialPort>, NameType)> {
    use serialport::*;

    let settings = SerialPortSettings {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        flow_control: FlowControl::None,
        parity: Parity::None,
        stop_bits: StopBits::One,
        timeout: std::time::Duration::from_millis(500),
    };

    let mut ser = serialport::open_with_settings(port, &settings)?;
    debug!("Resetting port {}", port.display());
    reset_device(&mut ser)?;
    std::thread::sleep(std::time::Duration::from_millis(2_500));
    debug!("Flushing serial port");
    flush_device(&mut ser)?;
    debug!("Getting device name");
    let name = get_device_name(&mut ser).map_err(anyhow::Error::from)?;
    Ok((ser, name))
}
