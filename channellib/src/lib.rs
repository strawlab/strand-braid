#[derive(thiserror::Error, Debug)]
#[error("chanellib receive error")]
pub struct RecvError(crossbeam_channel::RecvError);

#[derive(thiserror::Error, Debug)]
#[error("chanellib receive timeout error")]
pub struct RecvTimeoutError(crossbeam_channel::RecvTimeoutError);

impl RecvTimeoutError {
    #[inline(always)]
    pub fn is_timeout(&self) -> bool {
        self.0.is_timeout()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TryRecvError {
    #[error("chanellib try receive error empty")]
    Empty,
    #[error("chanellib try receive error disconnected")]
    Disconnected,
}

#[derive(thiserror::Error)]
#[error("chanellib send error")]
pub struct SendError<T>(crossbeam_channel::SendError<T>);

impl<T> std::fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "channellib::SendError")
    }
}

// ------

pub struct Receiver<T>(crossbeam_channel::Receiver<T>);

impl<T> Receiver<T> {
    #[inline(always)]
    pub fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().map_err(|e| RecvError(e))
    }

    #[inline(always)]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv().map_err(|e| match e {
            crossbeam_channel::TryRecvError::Empty => TryRecvError::Empty,
            crossbeam_channel::TryRecvError::Disconnected => TryRecvError::Disconnected,
        })
    }

    #[inline(always)]
    pub fn recv_timeout(&self, dur: std::time::Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(dur).map_err(|e| RecvTimeoutError(e))
    }
}

pub struct Sender<T>(crossbeam_channel::Sender<T>);

impl<T> Sender<T> {
    #[inline(always)]
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        self.0.send(msg).map_err(|e| SendError(e))
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
