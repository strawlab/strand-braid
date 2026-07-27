// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    io::Write,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

use h264_rtp::{AccessUnit, H264Payloader, RtpSessionConfig};

use crate::Result;

/// How often to log the measured-vs-target bitrate comparison.
const STATS_INTERVAL: Duration = Duration::from_secs(5);

/// Runs on its own thread for the lifetime of the stream. Owns the RTP session
/// state (SSRC, sequence counter, 90 kHz timestamp base via [`H264Payloader`])
/// and the UDP socket, both of which must survive an encoder respawn on
/// `set_bitrate`: the sender thread is independent of whichever encoder is
/// currently producing access units into `au_rx`.
///
/// Also independently monitors the achieved bitrate: `target_bitrate_bps` is
/// updated by the feeder thread whenever it (re)configures the encoder, and
/// every [`STATS_INTERVAL`] this thread logs it alongside the encoded byte
/// rate it measured itself straight off the access units it packetized — a
/// check on the encoder's rate control that doesn't rely on the encoder's own
/// bookkeeping.
pub(crate) fn run_sender(
    dest: SocketAddr,
    rtp_cfg: RtpSessionConfig,
    au_rx: Receiver<AccessUnit>,
    dump_annexb: Option<PathBuf>,
    target_bitrate_bps: Arc<AtomicU32>,
) -> Result<()> {
    let bind_addr: SocketAddr = match dest {
        SocketAddr::V4(_) => "0.0.0.0:0".parse().unwrap(),
        SocketAddr::V6(_) => "[::]:0".parse().unwrap(),
    };
    let socket = UdpSocket::bind(bind_addr)?;
    let mut payloader = H264Payloader::new(rtp_cfg);
    let mut dump_file = match dump_annexb {
        Some(path) => Some(std::fs::File::create(path)?),
        None => None,
    };

    let mut window_bytes: u64 = 0;
    let mut window_start = Instant::now();

    loop {
        let au = match au_rx.recv_timeout(STATS_INTERVAL) {
            Ok(au) => au,
            Err(RecvTimeoutError::Timeout) => {
                log_bitrate_stats(&mut window_bytes, &mut window_start, &target_bitrate_bps);
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        if let Some(f) = dump_file.as_mut() {
            for nal in &au.nals {
                f.write_all(&[0, 0, 0, 1])?;
                f.write_all(nal)?;
            }
        }
        window_bytes += encoded_bytes(&au);
        payloader.packetize(&au, &mut |packet| {
            socket.send_to(packet, dest)?;
            Ok(())
        })?;

        if window_start.elapsed() >= STATS_INTERVAL {
            log_bitrate_stats(&mut window_bytes, &mut window_start, &target_bitrate_bps);
        }
    }
    Ok(())
}

/// Total encoded bytes (all NALs) of one access unit, i.e. the video payload
/// the encoder produced before RTP/UDP/IP framing overhead.
fn encoded_bytes(au: &AccessUnit) -> u64 {
    au.nals.iter().map(|nal| nal.len() as u64).sum()
}

/// Bits per second implied by `bytes` sent over `elapsed`. `0` if `elapsed`
/// is zero, rather than dividing by zero.
fn measured_bps(bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs == 0.0 {
        0.0
    } else {
        bytes as f64 * 8.0 / secs
    }
}

fn log_bitrate_stats(
    window_bytes: &mut u64,
    window_start: &mut Instant,
    target_bitrate_bps: &Arc<AtomicU32>,
) {
    let elapsed = window_start.elapsed();
    let measured_kbps = measured_bps(*window_bytes, elapsed) / 1000.0;
    let target_kbps = target_bitrate_bps.load(Ordering::Acquire) as f64 / 1000.0;
    tracing::trace!(measured_kbps, target_kbps, "RTP encoder output bitrate");
    *window_bytes = 0;
    *window_start = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_bps_is_zero_for_zero_elapsed() {
        assert_eq!(measured_bps(1_000_000, Duration::ZERO), 0.0);
    }

    #[test]
    fn measured_bps_matches_known_rate() {
        // 500_000 bytes in 5 seconds = 4_000_000 bits in 5s = 800_000 bps.
        assert_eq!(measured_bps(500_000, Duration::from_secs(5)), 800_000.0);
    }

    #[test]
    fn encoded_bytes_sums_all_nals() {
        let au = AccessUnit {
            nals: vec![vec![0u8; 10], vec![0u8; 20], vec![0u8; 5]],
            is_keyframe: true,
            pts: Duration::ZERO,
        };
        assert_eq!(encoded_bytes(&au), 35);
    }
}
