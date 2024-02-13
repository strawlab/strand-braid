use std::net::UdpSocket;
use tracing::{error, warn};

use eyre::Result;

pub(crate) enum DatagramSocket {
    Udp(UdpSocket),
    #[cfg(feature = "flydra-uds")]
    Uds(unix_socket::UnixDatagram),
}

impl std::fmt::Debug for DatagramSocket {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DatagramSocket::Udp(s) => writeln!(fmt, "DatagramSocket::Udp({s:?})"),
            #[cfg(feature = "flydra-uds")]
            DatagramSocket::Uds(s) => writeln!(fmt, "DatagramSocket::Uds({:?})", s),
        }
    }
}

macro_rules! do_send {
    ($sock:expr, $data:expr) => {{
        match $sock.send(&$data) {
            Ok(sz) => {
                if sz != $data.len() {
                    eyre::bail!("incomplete send");
                }
            }
            Err(err) => match err.kind() {
                std::io::ErrorKind::WouldBlock => {
                    warn!("WouldBlock: dropping socket data");
                }
                std::io::ErrorKind::ConnectionRefused => {
                    warn!("ConnectionRefused: dropping socket data");
                }
                _ => {
                    error!("error sending socket data: {:?}", err);
                    return Err(err.into());
                }
            },
        }
    }};
}

impl DatagramSocket {
    pub(crate) fn send_complete(&self, x: &[u8]) -> Result<()> {
        use DatagramSocket::*;
        match self {
            Udp(s) => do_send!(s, x),
            #[cfg(feature = "flydra-uds")]
            Uds(s) => do_send!(s, x),
        }
        Ok(())
    }
}
