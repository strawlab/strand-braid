use crate::{std, Result, serialport, ascii};

pub(crate) fn serial_handshake(port: &std::path::Path) -> Result<ascii::String> {
    serial_handshake_no_defaults(port, 2, false)
}


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

fn crc8maxim(s: &[u8]) -> u8 {
    let mut crc = 0;
    for c in s {
        crc = _crc8maxim(crc,c);
    }
    crc
}

enum UdevError {
    CrcFail,
    NameNotSet,
    Io(std::io::Error),
    Ascii(ascii::Error),
}

impl From<std::io::Error> for UdevError {
    fn from(orig: std::io::Error) -> Self {
        UdevError::Io(orig)
    }
}

impl From<ascii::Error> for UdevError {
    fn from(orig: ascii::Error) -> Self {
        UdevError::Ascii(orig)
    }
}

fn get_device_name(device: &mut Box<dyn serialport::SerialPort>)
    -> std::result::Result<ascii::String,UdevError>
{
    let maxlen=8;

    use std::io::{Write, Read};
    device.write(b"N?")?;

    let len = maxlen+2;

    let mut buf = vec![0; len];
    device.read_exact(&mut buf)?;
    let name_and_crc = &buf[..len];
    trace!("get_device_name read {} bytes: {:?}", len, name_and_crc);
    let name = &name_and_crc[..maxlen];
    let crc_buf = &name_and_crc[maxlen..];
    if crc_buf.len() != 2 {
        return Err(UdevError::CrcFail);
    }
    let expected_crc = std::str::from_utf8(crc_buf).expect("from utf8");
    trace!("expected CRC: {:?}", expected_crc);

    let mut some_ascii = false;
    for byte in name.iter() {
        if byte.is_ascii() {
            some_ascii = true;
            break;
        }
    }

    if some_ascii {
        let computed_crc = format!("{:X}",crc8maxim(name));
        trace!("computed CRC: {:?}", computed_crc);
        if &computed_crc == expected_crc {
            Ok(ascii::String::from_vec(name.to_vec())?)
        } else {
            Err(UdevError::CrcFail)
        }
    } else {
        Err(UdevError::NameNotSet)
    }

}

fn serial_handshake_no_defaults(port: &std::path::Path, mut nretries: u8, error: bool) -> Result<ascii::String> {
    let mut name: Option<ascii::String> = None;

    use serialport::*;

    let settings = SerialPortSettings {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        flow_control: FlowControl::None,
        parity: Parity::None,
        stop_bits: StopBits::One,
        timeout: std::time::Duration::from_millis(10_000),
    };

    while nretries > 0 {
        let mut ser = serialport::open_with_settings(port, &settings)?;
        reset_device(&mut ser)?;
        std::thread::sleep(std::time::Duration::from_millis(2_500));
        flush_device(&mut ser)?;
        match get_device_name(&mut ser) {
            Ok(my_name) => {
                name = Some(my_name);
                flush_device(&mut ser)?;
                break;
            },
            Err(e) => {
                match e {
                    UdevError::NameNotSet => {
                        // repeat again
                    }
                    UdevError::CrcFail => {
                        return Err(format_err!("crc error"));
                    }
                    UdevError::Io(ioe) => {
                        return Err(ioe.into());
                    }
                    UdevError::Ascii(ae) => {
                        return Err(ae.into());
                    }
                }
            }
        }

        nretries -= 1;
    }

    match name {
        Some(name) => {
            Ok(name)
        },
        None => {
            match error {
                true => Err(format_err!("no serial")),
                false => Ok(ascii::String::empty())
            }
        },
    }

}
