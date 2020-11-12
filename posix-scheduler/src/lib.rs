extern crate libc;

use std::io::{Result, Error};

include!(concat!(env!("OUT_DIR"), "/consts.rs"));

macro_rules! syscall {
    ($ex:expr) => {{
        let result = unsafe { $ex };
        if result == -1 {
            return Err(Error::last_os_error())
        }
        result
    }}
}

/// Get the scheduling policy
pub fn sched_getscheduler(pid: libc::pid_t) -> Result<libc::c_int> {
    Ok(syscall!(libc::sched_getscheduler(pid)))
}

/// Set the scheduling policy and static scheduling priority
pub fn sched_setscheduler(pid: libc::pid_t, policy: libc::c_int, priority: libc::c_int) -> Result<()> {
    let sched_params = libc::sched_param {
        sched_priority: priority,
    };

    syscall!(libc::sched_setscheduler(pid, policy, &sched_params));
    Ok(())
}

/// Get the program scheduling priority
pub fn getpriority(which: libc::c_uint, who: libc::id_t) -> Result<libc::c_int> {
    Ok(syscall!(libc::getpriority(which, who)))
}

/// Set the program scheduling priority
pub fn setpriority(which: libc::c_uint, who: libc::id_t, prio: libc::c_int) -> Result<()> {
    syscall!(libc::setpriority(which, who, prio));
    Ok(())
}

// TODO: wrap sched_setaffinity() and sched_getaffinity()
