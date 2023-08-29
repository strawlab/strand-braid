#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access)
)]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[derive(thiserror::Error, Debug)]
#[error("chanellib receive error")]
pub struct RecvError {
    #[from]
    source: crossbeam_channel::RecvError,
    #[cfg(feature = "backtrace")]
    pub backtrace: Backtrace,
}

#[derive(thiserror::Error, Debug)]
#[error("chanellib receive timeout error")]
pub struct RecvTimeoutError {
    #[from]
    source: crossbeam_channel::RecvTimeoutError,
    #[cfg(feature = "backtrace")]
    pub backtrace: Backtrace,
}

impl RecvTimeoutError {
    #[inline(always)]
    pub fn is_timeout(&self) -> bool {
        self.source.is_timeout()
    }
}

#[derive(thiserror::Error, Debug)]
#[error("chanellib try receive error")]
pub struct TryRecvError {
    #[from]
    source: crossbeam_channel::TryRecvError,
    #[cfg(feature = "backtrace")]
    pub backtrace: Backtrace,
}

impl TryRecvError {
    pub fn inner(self) -> crossbeam_channel::TryRecvError {
        self.source
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.source, crossbeam_channel::TryRecvError::Empty)
    }

    pub fn is_disconnected(&self) -> bool {
        matches!(self.source, crossbeam_channel::TryRecvError::Disconnected)
    }
}

#[derive(thiserror::Error)]
#[error("chanellib send error")]
pub struct SendError<T> {
    inner: crossbeam_channel::SendError<T>,
    #[cfg(feature = "backtrace")]
    pub backtrace: Backtrace,
}

impl<T> std::fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "channellib::SendError")
    }
}

// ------

pub struct Receiver<T>(crossbeam_channel::Receiver<T>);

impl<T> Receiver<T> {
    pub fn into_inner(self) -> crossbeam_channel::Receiver<T> {
        self.0
    }

    #[inline(always)]
    pub fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().map_err(Into::into)
    }

    #[inline(always)]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv().map_err(Into::into)
    }

    #[inline(always)]
    pub fn recv_timeout(&self, dur: std::time::Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(dur).map_err(Into::into)
    }
}

pub struct Sender<T>(crossbeam_channel::Sender<T>);

impl<T> Sender<T> {
    pub fn into_inner(self) -> crossbeam_channel::Sender<T> {
        self.0
    }

    #[inline(always)]
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        self.0.send(msg).map_err(|e| SendError {
            inner: e,
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        })
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.0.is_full()
    }
}

impl<T> Clone for Sender<T> {
    #[inline(always)]
    fn clone(&self) -> Sender<T> {
        Sender(self.0.clone())
    }
}

#[inline(always)]
pub fn bounded<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = crossbeam_channel::bounded(cap);
    (Sender(tx), Receiver(rx))
}

#[inline(always)]
pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (Sender(tx), Receiver(rx))
}
