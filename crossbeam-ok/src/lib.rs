#[macro_use]
extern crate log;

/// convert a `Result<T,E>` into `Option<T>`, dropping the error E
///
/// The implementation is currently identical to `std::result::Result::ok()` but
/// could be used in the future to panic or log a warning.
pub trait CrossbeamOk<T> {
    fn cb_ok(self) -> Option<T>;
}

impl<T, E> CrossbeamOk<T> for std::result::Result<T, channellib::SendError<E>> {
    fn cb_ok(self) -> Option<T> {
        match self {
            Ok(val) => Some(val),
            Err(e) => {
                warn!("ignoring {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        use crate::CrossbeamOk;
        let (tx, _rx) = crossbeam_channel::unbounded();
        tx.send(123u8).cb_ok();
    }
}
