// Copyright (C) 2020 Andrew Straw <strawman@astraw.com>
// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use glib;
use glib::prelude::*;
use glib::subclass;
use glib::subclass::prelude::*;
use gst;
use gst::prelude::*;
use gst::subclass::prelude::*;

/// The data format as emitted by the nvarguscamerasrc component.
///
/// The best online reference I could find to this definition
/// is [here](https://forums.developer.nvidia.com/t/nvarguscamerasrc-buffer-metadata-is-missing/77676/29).
#[repr(C)]
struct AuxData {
    frame_num: i64,
    timestamp: i64,
}

// Struct containing all the element data
struct NvArgusTime {
    srcpad: gst::Pad,
    sinkpad: gst::Pad,
}

lazy_static! {
    static ref CAT: gst::DebugCategory = gst::DebugCategory::new(
        "nvargustime",
        gst::DebugColorFlags::empty(),
        Some("nvargustime Element"),
    );
}

impl NvArgusTime {
    // After creating of our two pads set all the functions on them
    //
    // Each function is wrapped in catch_panic_pad_function(), which will
    // - Catch panics from the pad functions and instead of aborting the process
    //   it will simply convert them into an error message and poison the element
    //   instance
    // - Extract our NvArgusTime struct from the object instance and pass it to us
    //
    // Details about what each function is good for is next to each function definition
    fn set_pad_functions(sinkpad: &gst::Pad, _srcpad: &gst::Pad) {
        unsafe {
            sinkpad.set_chain_function(|pad, parent, buffer| {
                NvArgusTime::catch_panic_pad_function(
                    parent,
                    || Err(gst::FlowError::Error),
                    |identity, element| identity.sink_chain(pad, element, buffer),
                )
            });
        }
    }

    // Called whenever a new buffer is passed to our sink pad. Here buffers should be processed and
    // whenever some output buffer is available have to push it out of the source pad.
    // Here we just pass through all buffers directly
    //
    // See the documentation of gst::Buffer and gst::BufferRef to see what can be done with
    // buffers.
    fn sink_chain(
        &self,
        pad: &gst::Pad,
        _element: &gst::Element,
        inbuf_orig: gst::Buffer,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        gst_log!(CAT, obj: pad, "Handling buffer {:?}", inbuf_orig);

        // Could we prevent this? (See comments below about why it is needed
        // now.)
        let buffer = inbuf_orig.copy();

        {
            // this will call `std::mem::forget(inbuf_orig)`
            let bufptr: *const gst_sys::GstBuffer = unsafe { inbuf_orig.into_ptr() };

            let minibufptr = bufptr as *mut gst_sys::GstMiniObject;
            let quark = unsafe {
                glib_sys::g_quark_from_static_string(b"GstBufferMetaData\0".as_ptr() as *const _)
            };
            // Cast the GstMiniObject to our AuxData type so we can read the values.
            let meta =
                unsafe { gst_sys::gst_mini_object_get_qdata(minibufptr, quark) } as *const AuxData;
            if meta.is_null() {
                panic!("unable to get GstBufferMetaData quark");
            }
            let (frame_num, timestamp) = unsafe { ((*meta).frame_num, (*meta).timestamp) };
            println!(
                "argustim: Acquired Frame: {}, time {}",
                frame_num, timestamp
            );

            // Above, `into_ptr()` calls `std::mem::forget()`. So, not to
            // leak, we need to deallocate the buffer. TODO: turn this back
            // into a rust object and prevent the copy above.
            unsafe { gst_sys::gst_mini_object_unref(minibufptr) };
        }

        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // unsafe{ libc::clock_gettime(CLOCK_MONOTONIC_RAW, &mut ts); }
        unsafe {
            libc::clock_gettime(CLOCK_MONOTONIC, &mut ts);
        }
        let tsc = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
        // println!("argustim: kernel time:            {} {}", ts.tv_sec, ts.tv_nsec);
        println!("argustim: kernel time:             {}", tsc);

        let pts = buffer.get_pts();

        println!("argustim: buffer PTS:   {:?}", pts);

        self.srcpad.push(buffer)
    }
}

const CLOCK_MONOTONIC: i32 = 1;
// const CLOCK_MONOTONIC_RAW: i32 = 4;

// This trait registers our type with the GObject object system and
// provides the entry points for creating a new instance and setting
// up the class data
impl ObjectSubclass for NvArgusTime {
    const NAME: &'static str = "NvArgusTime";
    type ParentType = gst::Element;
    type Instance = gst::subclass::ElementInstanceStruct<Self>;
    type Class = subclass::simple::ClassStruct<Self>;

    // This macro provides some boilerplate.
    glib_object_subclass!();

    // Called when a new instance is to be created. We need to return an instance
    // of our struct here and also get the class struct passed in case it's needed
    fn with_class(klass: &subclass::simple::ClassStruct<Self>) -> Self {
        // Create our two pads from the templates that were registered with
        // the class
        let templ = klass.get_pad_template("sink").unwrap();
        let sinkpad = gst::Pad::builder_with_template(&templ, Some("sink")).build();
        let templ = klass.get_pad_template("src").unwrap();
        let srcpad = gst::Pad::builder_with_template(&templ, Some("src")).build();

        // And then set all our pad functions for handling anything that happens
        // on these pads
        NvArgusTime::set_pad_functions(&sinkpad, &srcpad);

        // Return an instance of our struct and also include our debug category here.
        // The debug category will be used later whenever we need to put something
        // into the debug logs
        Self { srcpad, sinkpad }
    }

    // Called exactly once when registering the type. Used for
    // setting up metadata for all instances, e.g. the name and
    // classification and the pad templates with their caps.
    //
    // Actual instances can create pads based on those pad templates
    // with a subset of the caps given here.
    fn class_init(klass: &mut subclass::simple::ClassStruct<Self>) {
        // Set the element specific metadata. This information is what
        // is visible from gst-inspect-1.0 and can also be programatically
        // retrieved from the gst::Registry after initial registration
        // without having to load the plugin in memory.
        klass.set_metadata(
            "Nvidia Argus Time",
            "Generic",
            "Does nothing with the data",
            "Andrew Straw <strawman@astraw.com>",
        );

        // Create and add pad templates for our sink and source pad. These
        // are later used for actually creating the pads and beforehand
        // already provide information to GStreamer about all possible
        // pads that could exist for this type.

        // Our element can accept any possible caps on both pads
        let caps = gst::Caps::new_any();
        let src_pad_template = gst::PadTemplate::new(
            "src",
            gst::PadDirection::Src,
            gst::PadPresence::Always,
            &caps,
        )
        .unwrap();
        klass.add_pad_template(src_pad_template);

        let sink_pad_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Always,
            &caps,
        )
        .unwrap();
        klass.add_pad_template(sink_pad_template);
    }
}

// Implementation of glib::Object virtual methods
impl ObjectImpl for NvArgusTime {
    // This macro provides some boilerplate
    glib_object_impl!();

    // Called right after construction of a new instance
    fn constructed(&self, obj: &glib::Object) {
        // Call the parent class' ::constructed() implementation first
        self.parent_constructed(obj);

        // Here we actually add the pads we created in NvArgusTime::new() to the
        // element so that GStreamer is aware of their existence.
        let element = obj.downcast_ref::<gst::Element>().unwrap();
        element.add_pad(&self.sinkpad).unwrap();
        element.add_pad(&self.srcpad).unwrap();
    }
}

// Implementation of gst::Element virtual methods
impl ElementImpl for NvArgusTime {
    // Called whenever the state of the element should be changed. This allows for
    // starting up the element, allocating/deallocating resources or shutting down
    // the element again.
    fn change_state(
        &self,
        element: &gst::Element,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst_trace!(CAT, obj: element, "Changing state {:?}", transition);

        // Call the parent class' implementation of ::change_state()
        self.parent_change_state(element, transition)
    }
}

// Registers the type for our element, and then registers in GStreamer under
// the name "nvargustime" for being able to instantiate it via e.g.
// gst::ElementFactory::make().
pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "nvargustime",
        gst::Rank::None,
        NvArgusTime::get_type(),
    )
}
