// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    io::Write,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    sync::mpsc::Receiver,
};

use h264_rtp::{AccessUnit, H264Payloader, RtpSessionConfig};

use crate::Result;

/// Runs on its own thread for the lifetime of the stream. Owns the RTP session
/// state (SSRC, sequence counter, 90 kHz timestamp base via [`H264Payloader`])
/// and the UDP socket, both of which must survive an encoder respawn on
/// `set_bitrate`: the sender thread is independent of whichever encoder is
/// currently producing access units into `au_rx`.
pub(crate) fn run_sender(
    dest: SocketAddr,
    rtp_cfg: RtpSessionConfig,
    au_rx: Receiver<AccessUnit>,
    dump_annexb: Option<PathBuf>,
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

    for au in au_rx {
        if let Some(f) = dump_file.as_mut() {
            for nal in &au.nals {
                f.write_all(&[0, 0, 0, 1])?;
                f.write_all(nal)?;
            }
        }
        payloader.packetize(&au, &mut |packet| {
            socket.send_to(packet, dest)?;
            Ok(())
        })?;
    }
    Ok(())
}
