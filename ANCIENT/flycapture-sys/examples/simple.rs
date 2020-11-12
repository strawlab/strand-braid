extern crate flycapture_sys as ffi;

use ffi::{fc2Context, fc2CreateContext, fc2GetNumOfCameras, fc2Error};

macro_rules! fc2try {
    ($x:expr) => {
        match unsafe { $x } {
            fc2Error::FC2_ERROR_OK => {},
            e => { return Err(e); },
        }
    }
}

struct FlycapContext {
    ctx: fc2Context,
}

impl FlycapContext {
    fn new() -> Result<FlycapContext,fc2Error> {
        let mut ctx: fc2Context = std::ptr::null_mut();
        fc2try!(fc2CreateContext(&mut ctx));
        Ok(FlycapContext {ctx: ctx})
    }
}

impl Drop for FlycapContext {
    fn drop(&mut self) {
        match unsafe { ffi::fc2DestroyContext(self.ctx) } {
            fc2Error::FC2_ERROR_OK => {},
            _ => { panic!("Error during fc2DestroyContext()."); },
        }
    }
}

fn main() {
    let ctx = FlycapContext::new().unwrap();
    let mut n_cams = 0;

    match unsafe { fc2GetNumOfCameras(ctx.ctx, &mut n_cams) } {
        fc2Error::FC2_ERROR_OK => {},
        e => { panic!("could not get number of cameras: {:?}",e); },
    }

    println!("n_cams: {}", n_cams);
}
