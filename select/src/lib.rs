use std::os::unix::io::RawFd;

#[derive(Debug)]
pub enum Error {
    Errno(libc::c_int),
    Timeout,
}

pub fn block_or_timeout(fd: RawFd, timeout_ms: u32) -> Result<(),Error> {
    let mut timeout = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: (timeout_ms % 1000 * 1000) as libc::suseconds_t,
    };

    unsafe {
        let mut set: libc::fd_set = std::mem::zeroed();
        libc::FD_SET(fd, &mut set);
        let rc = libc::select(fd + 1, &mut set as *mut _, 0 as *mut _, 0 as *mut _, &mut timeout);
        if rc < 0 {
            Err(Error::Errno(std::io::Error::last_os_error().raw_os_error().unwrap()))
        } else if rc == 0 {
            Err(Error::Timeout)
        } else {
            Ok(())
        }
    }
}
