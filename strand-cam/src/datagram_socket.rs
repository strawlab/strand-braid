#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use std::net::UdpSocket;
use tracing::{error, warn};

use crate::StrandCamError;

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
                    return Err(StrandCamError::IncompleteSend(
                        #[cfg(feature = "backtrace")]
                        Backtrace::capture(),
                    ));
                }
            }
            Err(err) => {
                if std::io::ErrorKind::WouldBlock == err.kind() {
                    warn!("dropping socket data");
                } else {
                    error!("error sending socket data: {:?}", err);
                    return Err(err.into());
                }
            }
        }
    }};
}

impl DatagramSocket {
    pub(crate) fn send_complete(&self, x: &[u8]) -> Result<(), StrandCamError> {
        use DatagramSocket::*;
        match self {
            Udp(s) => do_send!(s, x),
            #[cfg(feature = "flydra-uds")]
            Uds(s) => do_send!(s, x),
        }
        Ok(())
    }
}
