extern crate libc;

use libc::c_int;

extern "C" {
    pub fn get_policy_SCHED_OTHER() -> c_int;
    pub fn get_policy_SCHED_FIFO() -> c_int;
    pub fn get_policy_SCHED_RR() -> c_int;
}

#[cfg(feature="linux")]
extern "C" {
    pub fn get_policy_SCHED_BATCH() -> c_int;
    pub fn get_policy_SCHED_IDLE() -> c_int;
}
