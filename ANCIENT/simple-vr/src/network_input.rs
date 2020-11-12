use std::sync::{Arc, Mutex};
use rustc_serialize::json;
use std::net::UdpSocket;

use super::config;
use super::tracker::Tracker;
use super::observation::Observation;

pub struct NetworkApp {
    tracker: Arc<Mutex<Tracker>>,
    socket: UdpSocket,
}

macro_rules! net_try {
    ($x:expr) => {
        match $x {
            Ok(x) => x,
            Err(y) => {
                error!("{:?}",y);
                return true
            }
        }
    }
}

impl NetworkApp {
    pub fn new(tracker: Arc<Mutex<Tracker>>, cfg: &config::NetworkListenerConfig) -> NetworkApp {
        let addr: &str = &cfg.socket_addr;
        let socket = UdpSocket::bind(addr).expect("binding socket");
        NetworkApp {
            tracker: tracker,
            socket: socket,
        }
    }

    pub fn network_step(&mut self) -> bool {
        // block on network input and return when it has something.
        let mut buf = [0; 1500];
        let (amt, _src) = net_try!(self.socket.recv_from(&mut buf));
        let buf = &mut buf[..amt];
        let s = net_try!(::std::str::from_utf8(buf));

        let data: Observation = net_try!(json::decode(s));

        self.tracker.lock().unwrap().handle_new_observation(&data);
        true
    }
}
