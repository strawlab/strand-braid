#[macro_use]
extern crate log;
extern crate byteorder;
extern crate chrono;
extern crate lstsq;
extern crate nalgebra as na;
extern crate serde;
extern crate serialport;
extern crate thread_control;

mod ascii;
mod datetime_conversion;

mod arduino_udev;
use crate::arduino_udev::serial_handshake;

use anyhow::{Context, Result};
use chrono::Duration;
use std::io::Write;

use crossbeam_channel::{Receiver, Sender};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Clone)]
pub struct ClockModel {
    pub gain: f64,
    pub offset: f64,
    pub residuals: f64,
    pub n_measurements: u64,
}

#[derive(Debug)]
pub struct TriggerClockInfoRow {
    // changes to this should update BraidMetadataSchemaTag
    pub start_timestamp: chrono::DateTime<chrono::Utc>,
    pub framecount: i64,
    pub tcnt: u8,
    pub stop_timestamp: chrono::DateTime<chrono::Utc>,
}

struct SerialThread {
    device: std::path::PathBuf,
    icr1_and_prescaler: Option<Icr1AndPrescaler>,
    version_check_done: bool,
    qi: u8,
    queries: BTreeMap<u8, chrono::DateTime<chrono::Utc>>,
    ser: Option<Box<dyn serialport::SerialPort>>,
    // raw_q: Sender<RawData>,
    // time_q: Sender<TimeData<R>>,
    outq: Receiver<Cmd>,
    vquery_time: chrono::DateTime<chrono::Utc>,
    last_time: chrono::DateTime<chrono::Utc>,
    past_data: Vec<(f64, f64)>,
    allow_requesting_clock_sync: bool,
    callback: Box<dyn FnMut(Option<ClockModel>)>,
    triggerbox_data_tx: Option<Sender<TriggerClockInfoRow>>,
}

// struct RawData {
//     send_timestamp: chrono::DateTime<chrono::Utc>,
//     pulsenumber: u32,
//     frac_u8: u8,
//     recv_timestamp: chrono::DateTime<chrono::Utc>,
// }

#[derive(Debug, Clone)]
pub enum Prescaler {
    Scale8,
    Scale64,
}

impl Prescaler {
    fn as_f64(&self) -> f64 {
        match self {
            Prescaler::Scale8 => 8.0,
            Prescaler::Scale64 => 64.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Icr1AndPrescaler {
    icr1: u16,
    prescaler: Prescaler,
}

#[derive(Debug, Clone)]
pub enum Cmd {
    Icr1AndPrescaler(Icr1AndPrescaler),
    StopPulsesAndReset,
    StartPulses,
}

impl SerialThread {
    fn new(
        device: std::path::PathBuf,
        // raw_q: Sender<RawData>,
        // time_q: Sender<TimeData<R>>,
        outq: Receiver<Cmd>,
        callback: Box<dyn FnMut(Option<ClockModel>)>,
        triggerbox_data_tx: Option<Sender<TriggerClockInfoRow>>,
    ) -> Result<Self> {
        let now = chrono::Utc::now();
        let vquery_time = now + Duration::seconds(1);
        Ok(Self {
            device,
            icr1_and_prescaler: None,
            version_check_done: false,
            qi: 0,
            queries: BTreeMap::new(),
            ser: None,
            // raw_q,
            outq: outq,
            vquery_time, // wait 1 second before first version query
            last_time: vquery_time + Duration::seconds(1), // and 1 second after version query
            past_data: Vec::new(),
            allow_requesting_clock_sync: false,
            callback,
            triggerbox_data_tx,
        })
    }

    fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        if let Some(ref mut ser) = self.ser {
            trace!("sending: \"{}\"", String::from_utf8_lossy(buf));
            for byte in buf.iter() {
                trace!("sending byte: {}", byte);
            }
            ser.write(buf)?;
        } else {
            panic!("serial device null")
        }
        Ok(())
    }

    fn run(&mut self, flag: thread_control::Flag, query_dt: std::time::Duration) -> Result<()> {
        let query_dt = Duration::from_std(query_dt)?;
        let name = serial_handshake(&self.device)
            .context(format!("opening device {}", self.device.display()))?;
        debug!("connected to device named {}", name);
        let mut now = chrono::Utc::now();

        let connect_time = now.clone();

        use serialport::*;

        let settings = SerialPortSettings {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            flow_control: FlowControl::None,
            parity: Parity::None,
            stop_bits: StopBits::One,
            timeout: std::time::Duration::from_millis(10),
        };
        self.ser = Some(serialport::open_with_settings(&self.device, &settings)?);

        let mut buf: Vec<u8> = Vec::new();
        let mut read_buf: Vec<u8> = vec![0; 100];
        let mut version_check_started = false;

        while flag.alive() {
            // handle new commands
            if self.version_check_done {
                loop {
                    match self.outq.recv_timeout(std::time::Duration::from_millis(0)) {
                        Ok(cmd_tup) => {
                            debug!("got command {:?}", cmd_tup);
                            match cmd_tup {
                                Cmd::Icr1AndPrescaler(new_value) => {
                                    self._set_icr1_and_prescaler(new_value)?;
                                }
                                Cmd::StopPulsesAndReset => {
                                    debug!(
                                        "will reset counters. dropping outstanding info requests."
                                    );
                                    self.allow_requesting_clock_sync = false;
                                    self.queries.clear();
                                    self.past_data.clear();
                                    (self.callback)(None);
                                    self.write(b"S0")?;
                                }
                                Cmd::StartPulses => {
                                    self.allow_requesting_clock_sync = true;
                                    self.write(b"S1")?;
                                }
                            }
                        }
                        Err(e) => {
                            if e.is_timeout() {
                                break;
                            } else {
                                return Err(e.into());
                            }
                        }
                    }
                }
            }

            // get all pending data
            if let Some(ref mut ser) = self.ser {
                // TODO: this could be made (much) more efficient. Right
                // now, we wake up every timeout duration and run the whole
                // cycle when no byte arrives.
                match ser.read(&mut read_buf) {
                    Ok(n_bytes_read) => {
                        for i in 0..n_bytes_read {
                            let byte = read_buf[i];
                            trace!(
                                "read byte {} (char {})",
                                byte,
                                String::from_utf8_lossy(&read_buf[i..i + 1])
                            );
                            buf.push(byte);
                        }
                    }
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::TimedOut => {}
                        _ => {
                            return Err(e.into());
                        }
                    },
                }
            } else {
                unreachable!();
            }

            // handle pending data
            buf = self._h(buf)?;

            now = chrono::Utc::now();

            if self.version_check_done {
                if self.allow_requesting_clock_sync
                    & (now.signed_duration_since(self.last_time) > query_dt)
                {
                    // request sample
                    debug!("making clock sample request. qi: {}, now: {}", self.qi, now);
                    self.queries.insert(self.qi, now);
                    let send_buf = ['P' as u8, self.qi];
                    self.write(&send_buf)?;
                    self.qi = self.qi.wrapping_add(1);
                    self.last_time = now;
                }
            } else {
                // request firmware version
                if !version_check_started {
                    if now >= self.vquery_time {
                        info!("checking firmware version");
                        self.write(b"V?")?;
                        version_check_started = true;
                        self.vquery_time = now;
                    }
                }

                // retry every second
                if now.signed_duration_since(self.vquery_time) > Duration::seconds(1) {
                    version_check_started = false;
                }
                // give up after 20 seconds
                if now.signed_duration_since(connect_time) > Duration::seconds(20) {
                    return Err(anyhow::anyhow!("no version response"));
                }
            }
        }
        info!("exiting run loop");
        Ok(())
    }

    fn _set_icr1_and_prescaler(&mut self, new_value: Icr1AndPrescaler) -> Result<()> {
        use byteorder::{ByteOrder, LittleEndian};

        let mut buf = [0, 0, 0];
        LittleEndian::write_u16(&mut buf[0..2], new_value.icr1);
        buf[2] = match &new_value.prescaler {
            Prescaler::Scale8 => '1' as u8,
            Prescaler::Scale64 => '2' as u8,
        };

        self.icr1_and_prescaler = Some(new_value);

        self.write(b"T=")?;
        self.write(&buf)?;
        Ok(())
    }

    fn _handle_returned_timestamp(&mut self, qi: u8, pulsenumber: u32, count: u16) -> Result<()> {
        debug!(
            "got returned timestamp with qi: {}, pulsenumber: {}, count: {}",
            qi, pulsenumber, count
        );
        let now = chrono::Utc::now();
        while self.queries.len() > 50 {
            self.queries.clear();
            error!("too many outstanding queries");
        }

        let send_timestamp = match self.queries.remove(&qi) {
            Some(send_timestamp) => send_timestamp,
            None => {
                warn!("could not find original data for query {:?}", qi);
                return Ok(());
            }
        };
        trace!("this query has send_timestamp: {}", send_timestamp);

        let max_error = now.signed_duration_since(send_timestamp);
        if max_error > Duration::milliseconds(20) {
            warn!("clock sample took {:?}. Ignoring value.", max_error);
            return Ok(());
        }

        trace!("max_error: {:?}", max_error);

        let ino_time_estimate = now + (max_error / 2);

        match &self.icr1_and_prescaler {
            Some(s) => {
                let frac = count as f64 / s.icr1 as f64;
                debug_assert!(0.0 <= frac);
                debug_assert!(frac <= 1.0);
                let ino_stamp = na::convert(pulsenumber as f64 + frac);

                if let Some(ref tbox_tx) = self.triggerbox_data_tx {
                    // send our newly acquired data to be saved to disk
                    let to_save = TriggerClockInfoRow {
                        start_timestamp: send_timestamp,
                        framecount: pulsenumber as i64,
                        tcnt: (frac * 255.0) as u8,
                        stop_timestamp: now,
                    };
                    match tbox_tx.send(to_save) {
                        Ok(()) => {}
                        Err(e) => {
                            warn!("ignoring {}", e);
                        }
                    }
                }

                // delete old data
                while self.past_data.len() > 100 {
                    self.past_data.remove(0);
                }

                self.past_data.push((
                    ino_stamp,
                    datetime_conversion::datetime_to_f64(&ino_time_estimate),
                ));

                if self.past_data.len() >= 5 {
                    use na::{OMatrix, OVector, U2};

                    // fit time model
                    let mut a: Vec<f64> = Vec::with_capacity(self.past_data.len() * 2);
                    let mut b: Vec<f64> = Vec::with_capacity(self.past_data.len());

                    for row in self.past_data.iter() {
                        a.push(row.0);
                        a.push(1.0);
                        b.push(row.1);
                    }
                    let a = OMatrix::<f64, na::Dynamic, U2>::from_row_slice(&a);
                    let b = OVector::<f64, na::Dynamic>::from_row_slice(&b);

                    let epsilon = 1e-10;
                    let results = lstsq::lstsq(&a, &b, epsilon)
                        .map_err(|e| anyhow::anyhow!("lstsq err: {}", e))?;

                    let gain = results.solution[0];
                    let offset = results.solution[1];
                    let residuals = results.residuals;
                    let n_measurements = self.past_data.len() as u64;
                    let per_point_residual = residuals / n_measurements as f64;
                    // TODO only accept this if residuals less than some amount?
                    debug!(
                        "new: ClockModel{{gain: {}, offset: {}}}, per_point_residual: {}",
                        gain, offset, per_point_residual
                    );
                    (self.callback)(Some(ClockModel {
                        gain,
                        offset,
                        residuals,
                        n_measurements,
                    }));
                }

                // let frac_u8 = (frac * 255.0).round() as u8;
                // let recv_timestamp = now;
                // self.raw_q.send( RawData {send_timestamp, pulsenumber,
                //     frac_u8, recv_timestamp} )?;
            }
            None => {
                warn!("No clock measurements until framerate set.");
            }
        }
        Ok(())
    }

    fn _handle_version(&mut self, value: u8, _pulsenumber: u32, _count: u16) -> Result<()> {
        trace!("got returned version with value: {}", value);
        assert!(value == 14);
        self.vquery_time = chrono::Utc::now();
        self.version_check_done = true;
        info!("connected to triggerbox firmware version {}", value);
        Ok(())
    }

    fn _h(&mut self, buf: Vec<u8>) -> Result<Vec<u8>> {
        if buf.len() >= 3 {
            // header, length, checksum is minimum
            let mut valid_n_chars = None;

            let packet_type = buf[0] as char;
            let payload_len = buf[1];

            let min_valid_packet_size = 3 + payload_len as usize; // header (2) + payload + checksum (1)
            if buf.len() >= min_valid_packet_size {
                let expected_chksum = buf[2 + payload_len as usize];

                let check_buf = &buf[2..buf.len() - 1];
                let bytes = check_buf;
                let actual_chksum = bytes.iter().fold(0, |acc: u8, x| acc.wrapping_add(*x));

                if actual_chksum == expected_chksum {
                    trace!("checksum OK");
                    valid_n_chars = Some(bytes.len() + 3)
                } else {
                    return Err(anyhow::anyhow!("checksum mismatch"));
                }

                if (packet_type == 'P') | (packet_type == 'V') {
                    assert!(payload_len == 7);
                    let value = bytes[0];

                    use byteorder::{ByteOrder, LittleEndian};
                    let pulsenumber = LittleEndian::read_u32(&bytes[1..5]);
                    let count = LittleEndian::read_u16(&bytes[5..7]);

                    match packet_type {
                        'P' => self._handle_returned_timestamp(value, pulsenumber, count)?,
                        'V' => self._handle_version(value, pulsenumber, count)?,
                        _ => unreachable!(),
                    };
                }
            }

            if let Some(n_used_chars) = valid_n_chars {
                return Ok(buf[n_used_chars..].to_vec());
            }
        }
        Ok(buf)
    }
}

pub fn launch_background_thread(
    callback: Box<dyn FnMut(Option<ClockModel>) + Send>,
    device: std::path::PathBuf,
    cmd: Receiver<Cmd>,
    triggerbox_data_tx: Option<Sender<TriggerClockInfoRow>>,
    query_dt: std::time::Duration,
) -> Result<(thread_control::Control, std::thread::JoinHandle<()>)> {
    // let (raw_tx, raw_rx) = channellib::unbounded();

    let triggerbox_thread_builder =
        std::thread::Builder::new().name("triggerbox_comms".to_string());
    let (flag, control) = thread_control::make_pair();
    let triggerbox_thread_handle = triggerbox_thread_builder.spawn(move || {
        run_func(|| {
            let mut triggerbox =
                SerialThread::new(device, /*raw_tx,*/ cmd, callback, triggerbox_data_tx)?;
            triggerbox.run(flag, query_dt)
        });
    })?;

    Ok((control, triggerbox_thread_handle))
}

/// run a function returning Result<()> and handle errors.
// see https://github.com/withoutboats/failure/issues/76#issuecomment-347402383
fn run_func<F: FnOnce() -> Result<()>>(real_func: F) {
    // Decide which command to run, and run it, and print any errors.
    if let Err(err) = real_func() {
        let mut stderr = std::io::stderr();
        writeln!(stderr, "Error: {}", err).expect("unable to write error to stderr");
        for cause in err.chain() {
            writeln!(stderr, "Caused by: {}", cause).expect("unable to write error to stderr");
        }
        std::process::exit(1);
    }
}

fn get_rate(rate_ideal: f64, prescaler: Prescaler) -> (u16, f64) {
    let xtal = 16e6; // 16 MHz clock
    let base_clock = xtal / prescaler.as_f64();
    let new_top_ideal = base_clock / rate_ideal;
    let new_icr1_f64 = new_top_ideal.round();
    let new_icr1: u16 = if new_icr1_f64 > 0xFFFF as f64 {
        0xFFFF
    } else if new_icr1_f64 < 0.0 {
        0
    } else {
        new_icr1_f64 as u16
    };
    let rate_actual = base_clock / new_icr1 as f64;
    (new_icr1, rate_actual)
}

pub fn make_trig_fps_cmd(rate_ideal: f64) -> Cmd {
    let (icr1_8, rate_actual_8) = get_rate(rate_ideal, Prescaler::Scale8);
    let (icr1_64, rate_actual_64) = get_rate(rate_ideal, Prescaler::Scale64);

    let error_8 = (rate_ideal - rate_actual_8).abs();
    let error_64 = (rate_ideal - rate_actual_64).abs();

    let (icr1, _rate_actual, prescaler) = if error_8 < error_64 {
        (icr1_8, rate_actual_8, Prescaler::Scale8)
    } else {
        (icr1_64, rate_actual_64, Prescaler::Scale64)
    };

    Cmd::Icr1AndPrescaler(Icr1AndPrescaler { icr1, prescaler })
}
