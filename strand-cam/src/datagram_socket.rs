use std::net::UdpSocket;
use tracing::{error, warn};

use eyre::Result;

pub(crate) trait SendComplete {
    fn send_complete(&self, x: &[u8]) -> Result<()>;
}

impl SendComplete for UdpSocket {
    fn send_complete(&self, x: &[u8]) -> Result<()> {
        match self.send(&x) {
            Ok(sz) => {
                if sz != x.len() {
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
        Ok(())
    }
}
