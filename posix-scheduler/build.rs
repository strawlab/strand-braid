extern crate posix_scheduler_build;

use std::env;
use std::path::Path;
use std::fs::File;
use std::io::Write;

use posix_scheduler_build::*;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("consts.rs");
    let mut f = File::create(&dest_path).unwrap();

    let line = format!("pub const SCHED_OTHER: libc::c_int = {};", unsafe { get_policy_SCHED_OTHER() });
    f.write_all(line.as_bytes()).unwrap();

    #[cfg(feature="linux")]
    {
        let line = format!("pub const SCHED_BATCH: libc::c_int = {};", unsafe { get_policy_SCHED_BATCH() });
        f.write_all(line.as_bytes()).unwrap();

        let line = format!("pub const SCHED_IDLE: libc::c_int = {};", unsafe { get_policy_SCHED_IDLE() });
        f.write_all(line.as_bytes()).unwrap();
    }

    let line = format!("pub const SCHED_FIFO: libc::c_int = {};", unsafe { get_policy_SCHED_FIFO() });
    f.write_all(line.as_bytes()).unwrap();

    let line = format!("pub const SCHED_RR: libc::c_int = {};", unsafe { get_policy_SCHED_RR() });
    f.write_all(line.as_bytes()).unwrap();

}
