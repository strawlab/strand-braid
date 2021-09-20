// Copyright (C) 2020 Andrew Straw <strawman@astraw.com>
//
// Licensed under the BSD 2 Clause License. See LICENSE.txt.
//
// Copyright (C) 2018 Sebastian Dröge <sebastian@centricular.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use glib;
use glib::prelude::*;
use glib::subclass;
use glib::subclass::prelude::*;
use gst;
use gst::prelude::*;
use gst::subclass::prelude::*;

use crate::TagFamily;
use std::convert::TryInto;
use std::i32;
use std::sync::Mutex;

use ads_apriltag as apriltag;

const SRC_CAPS: &'static str = "text/x-csv";

// Property value storage
#[derive(Debug, Clone)]
struct Settings {
    family: TagFamily,
    maxhamming: i32,
    decimate: f32,
    blur: f32,
    refine_edges: bool,
}

const DEFAULT_FAMILY: TagFamily = TagFamily::Family36h11;
const DEFAULT_MAXHAMMING: i32 = 1;
const DEFAULT_DECIMATE: f32 = 2.0;
const DEFAULT_BLUR: f32 = 0.0;
const DEFAULT_REFINE_EDGES: bool = true;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            family: DEFAULT_FAMILY,
            maxhamming: DEFAULT_MAXHAMMING,
            decimate: DEFAULT_DECIMATE,
            blur: DEFAULT_BLUR,
            refine_edges: DEFAULT_REFINE_EDGES,
        }
    }
}

// Metadata for the properties
static PROPERTIES: [subclass::Property; 5] = [
    subclass::Property("family", |name| {
        glib::ParamSpec::enum_(
            name,
            "Family",
            "Tag family to use",
            TagFamily::static_type(),
            DEFAULT_FAMILY as i32,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("maxhamming", |name| {
        glib::ParamSpec::int(
            name,
            "maxhamming",
            "Detect tags with up to this many bit errors",
            0,
            std::i32::MAX,
            DEFAULT_MAXHAMMING,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("decimate", |name| {
        glib::ParamSpec::float(
            name,
            "Decimate",
            "Decimate input image by this factor",
            0.0,
            std::f32::MAX,
            DEFAULT_DECIMATE,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("blur", |name| {
        glib::ParamSpec::float(
            name,
            "Blur",
            "Apply low-pass blur to input; negative sharpens",
            std::f32::MIN,
            std::f32::MAX,
            DEFAULT_BLUR,
            glib::ParamFlags::READWRITE,
        )
    }),
    subclass::Property("refine-edges", |name| {
        glib::ParamSpec::boolean(
            name,
            "refine-edges",
            "Spend more time trying to align edges of tags",
            DEFAULT_REFINE_EDGES,
            glib::ParamFlags::READWRITE,
        )
    }),
    // TODO add threads, debug as properties?
];

// Stream-specific state, i.e. video format configuration
#[derive(Debug)]
struct State {
    write_headers: bool,
    video_info: gst_video::VideoInfo,
    inner: apriltag::Detector,
}

// Struct containing all the element data
struct AprilTagDetector {
    settings: Mutex<Settings>,
    srcpad: gst::Pad,
    sinkpad: gst::Pad,
    state: Mutex<Option<State>>,
}

lazy_static! {
    static ref CAT: gst::DebugCategory = gst::DebugCategory::new(
        "apriltagdetector",
        gst::DebugColorFlags::empty(),
        Some("AprilTagDetector Element"),
    );
}

fn add_family(td: &mut apriltag::Detector, settings: &Settings) {
    use TagFamily::*;
    let tf = match settings.family {
        Family36h11 => apriltag::Family::new_tag_36h11(),
        FamilyStandard41h12 => apriltag::Family::new_tag_standard_41h12(),
        Family16h5 => apriltag::Family::new_tag_16h5(),
        Family25h9 => apriltag::Family::new_tag_25h9(),
        FamilyCircle21h7 => apriltag::Family::new_tag_circle_21h7(),
        FamilyCircle49h12 => apriltag::Family::new_tag_circle_49h12(),
        FamilyCustom48h12 => apriltag::Family::new_tag_custom_48h12(),
        FamilyStandard52h13 => apriltag::Family::new_tag_standard_52h13(),
    };
    td.add_family_bits(tf, settings.maxhamming);

    gst_debug!(
        CAT,
        "set april tag detector family {}, maxhamming {}",
        settings.family,
        settings.maxhamming
    );
}

fn make_detector(settings: &Settings) -> apriltag::Detector {
    let mut td = apriltag::Detector::new();
    add_family(&mut td, &settings);

    let mut raw_td = td.as_mut();
    raw_td.quad_decimate = settings.decimate;
    raw_td.quad_sigma = settings.blur;
    raw_td.refine_edges = if settings.refine_edges { 1 } else { 0 };
    raw_td.decode_sharpening = 0.25;
    td
}

fn do_detections(
    in_frame: gst_video::VideoFrameRef<&gst::BufferRef>,
    td: &apriltag::Detector,
) -> apriltag::Zarray<apriltag::Detection> {
    use apriltag::ImageU8;

    // Keep the various metadata we need for working with the video frames in
    // local variables. This saves some typing below.
    let width = in_frame.width().try_into().unwrap();
    let height = in_frame.height().try_into().unwrap();
    let stride = in_frame.plane_stride()[0].try_into().unwrap();
    let data = in_frame.plane_data(0).unwrap();

    gst_debug!(
        CAT,
        "detecting with width {}, height {}, stride {}, data {:?}",
        width,
        height,
        stride,
        &data[..3]
    );

    let im = apriltag::ImageU8Borrowed::new(width, height, stride, data);

    // given our caps, this is a video/x-raw Gray8 video frame
    let result = td.detect(&im.inner());

    gst_debug!(CAT, "found {} point(s)", result.as_slice().len());

    result
}

use serde::Serialize;

// The center pixel of the detection is (h02,h12)
#[derive(Serialize)]
struct DetectionSerializer {
    // frame: usize,
    pts_nanoseconds: Option<u64>,
    id: i32,
    hamming: i32,
    decision_margin: f32,
    h00: f64,
    h01: f64,
    h02: f64,
    h10: f64,
    h11: f64,
    h12: f64,
    h20: f64,
    h21: f64,
    // no h22 because it is always 1.0
    family: String,
}

fn to_serializer(orig: &apriltag::Detection, pts: gst::ClockTime) -> DetectionSerializer {
    let h = orig.h();
    // We are not going to save h22, so (in debug builds) let's check it meets
    // our expectations.
    debug_assert!((h[8] - 1.0).abs() < 1e-16);
    DetectionSerializer {
        // frame,
        pts_nanoseconds: pts.nanoseconds(),
        id: orig.id(),
        hamming: orig.hamming(),
        decision_margin: orig.decision_margin(),
        h00: h[0],
        h01: h[1],
        h02: h[2],
        h10: h[3],
        h11: h[4],
        h12: h[5],
        h20: h[6],
        h21: h[7],
        family: orig.family_type().to_str().to_string(),
    }
}

fn to_csv_lines(
    detections: &[apriltag::Detection],
    write_headers: bool,
    pts: gst::ClockTime,
) -> gst::Buffer {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(write_headers)
        .from_writer(Vec::new());

    for d in detections.iter() {
        let d2 = to_serializer(&d, pts);
        wtr.serialize(d2).expect("serialize");
    }
    let my_bytes = wtr.into_inner().expect("into inner buffer");

    let mut buffer = gst::Buffer::with_size(my_bytes.len()).unwrap();
    {
        let buffer = buffer.get_mut().unwrap();
        let mut data = buffer.map_writable().unwrap();
        let mut dslice = data.as_mut_slice();

        // TODO: This makes a copy. Can we eliminate the copy?
        use bytes::BufMut;
        dslice.put(my_bytes.as_slice());
    }

    buffer
}

impl AprilTagDetector {
    // After creating of our two pads set all the functions on them
    //
    // Each function is wrapped in catch_panic_pad_function(), which will
    // - Catch panics from the pad functions and instead of aborting the process
    //   it will simply convert them into an error message and poison the element
    //   instance
    // - Extract our AprilTagDetector struct from the object instance and pass it to us
    //
    // Details about what each function is good for is next to each function definition
    fn set_pad_functions(sinkpad: &gst::Pad, srcpad: &gst::Pad) {
        unsafe {
            sinkpad.set_chain_function(|pad, parent, buffer| {
                AprilTagDetector::catch_panic_pad_function(
                    parent,
                    || Err(gst::FlowError::Error),
                    |april_tag, element| april_tag.sink_chain(pad, element, buffer),
                )
            });
            sinkpad.set_event_function(|pad, parent, event| {
                AprilTagDetector::catch_panic_pad_function(
                    parent,
                    || false,
                    |april_tag, element| april_tag.sink_event(pad, element, event),
                )
            });
            // sinkpad.set_query_function(|pad, parent, query| {
            //     AprilTagDetector::catch_panic_pad_function(
            //         parent,
            //         || false,
            //         |april_tag, element| april_tag.sink_query(pad, element, query),
            //     )
            // });

            srcpad.set_event_function(|pad, parent, event| {
                AprilTagDetector::catch_panic_pad_function(
                    parent,
                    || false,
                    |april_tag, element| april_tag.src_event(pad, element, event),
                )
            });
            srcpad.set_query_function(|pad, parent, query| {
                AprilTagDetector::catch_panic_pad_function(
                    parent,
                    || false,
                    |april_tag, element| april_tag.src_query(pad, element, query),
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
        element: &gst::Element,
        inbuf: gst::Buffer,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        gst_debug!(CAT, obj: pad, "Handling buffer {:?}", inbuf);
        let pts = inbuf.get_pts();

        let (detections, write_headers) = {
            let mut state_guard = self.state.lock().unwrap();
            let state = state_guard.as_mut().unwrap();

            // Map the input buffer as a VideoFrameRef. This is similar to directly mapping
            // the buffer with inbuf.map_readable() but in addition extracts various video
            // specific metadata and sets up a convenient data structure that directly gives
            // pointers to the different planes and has all the information about the raw
            // video frame, like width, height, stride, video format, etc.
            //
            // This fails if the buffer can't be read or is invalid in relation to the video
            // info that is passed here
            let in_frame = gst_video::VideoFrameRef::from_buffer_ref_readable(
                inbuf.as_ref(),
                &state.video_info,
            )
            .map_err(|_| {
                gst_element_error!(
                    element,
                    gst::CoreError::Failed,
                    ["Failed to map input buffer readable"]
                );
                gst::FlowError::Error
            })?;

            let write_headers = state.write_headers;

            let detections = do_detections(in_frame, &state.inner);
            if write_headers && detections.len() > 0 {
                // The headers will get written below, so we do not have to keep
                // writing them.
                state.write_headers = false;
            }

            (detections, write_headers)
            // drop state_guard here
        };

        let buffer = to_csv_lines(detections.as_slice(), write_headers, pts);

        self.srcpad.push(buffer).map_err(|err| {
            gst_error!(CAT, obj: element, "Failed to push buffer {:?}", err);
            err
        })?;

        Ok(gst::FlowSuccess::Ok)
    }

    // Called whenever an event arrives on the sink pad. It has to be handled accordingly and in
    // most cases has to be either passed to Pad::event_default() on this pad for default handling,
    // or Pad::push_event() on all pads with the opposite direction for direct forwarding.
    // Here we just pass through all events directly to the source pad.
    //
    // See the documentation of gst::Event and gst::EventRef to see what can be done with
    // events, and especially the gst::EventView type for inspecting events.
    fn sink_event(&self, pad: &gst::Pad, element: &gst::Element, event: gst::Event) -> bool {
        use gst::EventView;

        gst_log!(CAT, obj: pad, "Handling event {:?}", event);

        match event.view() {
            EventView::Caps(ev) => {
                let incaps = ev.get_caps();

                let video_info = match gst_video::VideoInfo::from_caps(incaps) {
                    Err(_) => {
                        // return Err(gst_loggable_error!(CAT, "Failed to parse input caps"))
                        panic!("Failed to parse input caps");
                    }
                    Ok(info) => info,
                };
                gst_log!(CAT, obj: pad, "Got video_info {:?}", video_info);

                {
                    let settings_guard = self.settings.lock().unwrap();

                    let mut state_guard = self.state.lock().unwrap();
                    *state_guard = Some(State {
                        write_headers: true,
                        video_info,
                        inner: make_detector(&settings_guard),
                    });
                }

                // let s = caps.get_structure(0).unwrap();
                // let framerate = match s.get_some::<gst::Fraction>("framerate") {

                // We send our own caps downstream
                let caps = gst::Caps::builder(SRC_CAPS).build();
                self.srcpad.push_event(gst::event::Caps::new(&caps))
            }
            _ => pad.event_default(Some(element), event),
        }
    }

    // // Called whenever a query is sent to the sink pad. It has to be answered if the element can
    // // handle it, potentially by forwarding the query first to the peer pads of the pads with the
    // // opposite direction, or false has to be returned. Default handling can be achieved with
    // // Pad::query_default() on this pad and forwarding with Pad::peer_query() on the pads with the
    // // opposite direction.
    // // Here we just forward all queries directly to the source pad's peers.
    // //
    // // See the documentation of gst::Query and gst::QueryRef to see what can be done with
    // // queries, and especially the gst::QueryView type for inspecting and modifying queries.
    // fn sink_query(
    //     &self,
    //     pad: &gst::Pad,
    //     _element: &gst::Element,
    //     query: &mut gst::QueryRef,
    // ) -> bool {
    //     gst_log!(CAT, obj: pad, "Handling query {:?}", query);
    //     self.srcpad.peer_query(query)
    // }

    // Called whenever an event arrives on the source pad. It has to be handled accordingly and in
    // most cases has to be either passed to Pad::event_default() on the same pad for default
    // handling, or Pad::push_event() on all pads with the opposite direction for direct
    // forwarding.
    // Here we just pass through all events directly to the sink pad.
    //
    // See the documentation of gst::Event and gst::EventRef to see what can be done with
    // events, and especially the gst::EventView type for inspecting events.
    fn src_event(&self, pad: &gst::Pad, _element: &gst::Element, event: gst::Event) -> bool {
        gst_log!(CAT, obj: pad, "Handling event {:?}", event);
        self.sinkpad.push_event(event)
    }

    // Called whenever a query is sent to the source pad. It has to be answered if the element can
    // handle it, potentially by forwarding the query first to the peer pads of the pads with the
    // opposite direction, or false has to be returned. Default handling can be achieved with
    // Pad::query_default() on this pad and forwarding with Pad::peer_query() on the pads with the
    // opposite direction.
    // Here we just forward all queries directly to the sink pad's peers.
    //
    // See the documentation of gst::Query and gst::QueryRef to see what can be done with
    // queries, and especially the gst::QueryView type for inspecting and modifying queries.
    fn src_query(
        &self,
        pad: &gst::Pad,
        _element: &gst::Element,
        query: &mut gst::QueryRef,
    ) -> bool {
        gst_log!(CAT, obj: pad, "Handling query {:?}", query);
        // TODO: should we somehow return "unknown" number of bytes here?
        self.sinkpad.peer_query(query)
    }
}

// This trait registers our type with the GObject object system and
// provides the entry points for creating a new instance and setting
// up the class data
impl ObjectSubclass for AprilTagDetector {
    const NAME: &'static str = "AprilTagDetector";
    type ParentType = gst::Element;
    type Instance = gst::subclass::ElementInstanceStruct<Self>;
    type Class = subclass::simple::ClassStruct<Self>;

    // This macro provides some boilerplate.
    glib_object_subclass!();

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
            "AprilTagDetector",
            "Filter/Analyzer/Video",
            "Detects and localizes April Tags in video",
            "Andrew Straw <strawman@astraw.com>",
        );

        // Create and add pad templates for our sink and source pad. These
        // are later used for actually creating the pads and beforehand
        // already provide information to GStreamer about all possible
        // pads that could exist for this type.

        // sink - take in gray8 frames

        let caps = gst::Caps::new_simple(
            "video/x-raw",
            &[
                ("format", &gst_video::VideoFormat::Gray8.to_str()),
                ("width", &gst::IntRange::<i32>::new(0, i32::MAX)),
                ("height", &gst::IntRange::<i32>::new(0, i32::MAX)),
                (
                    "framerate",
                    &gst::FractionRange::new(
                        gst::Fraction::new(0, 1),
                        gst::Fraction::new(i32::MAX, 1),
                    ),
                ),
            ],
        );
        let sink_pad_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Always,
            &caps,
        )
        .unwrap();
        klass.add_pad_template(sink_pad_template);

        // ------

        let caps = gst::Caps::new_simple(SRC_CAPS, &[]);
        let src_pad_template = gst::PadTemplate::new(
            "src",
            gst::PadDirection::Src,
            gst::PadPresence::Always,
            &caps,
        )
        .unwrap();
        klass.add_pad_template(src_pad_template);

        // ------

        // Install all our properties
        klass.install_properties(&PROPERTIES);
    }

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
        AprilTagDetector::set_pad_functions(&sinkpad, &srcpad);

        // Return an instance of our struct and also include our debug category here.
        // The debug category will be used later whenever we need to put something
        // into the debug logs
        Self {
            settings: Mutex::new(Default::default()),
            srcpad,
            sinkpad,
            state: Mutex::new(None),
        }
    }
}

// Implementation of glib::Object virtual methods
impl ObjectImpl for AprilTagDetector {
    // This macro provides some boilerplate
    glib_object_impl!();

    // Called whenever a value of a property is changed. It can be called
    // at any time from any thread.
    fn set_property(&self, obj: &glib::Object, id: usize, value: &glib::Value) {
        let prop = &PROPERTIES[id];

        let element = match obj.downcast_ref::<gst::Element>() {
            Some(e) => e,
            None => {
                return;
            }
        };

        let mut settings = self.settings.lock().unwrap();

        match *prop {
            subclass::Property("family", ..) => {
                settings.family = value.get_some().unwrap();
            }
            subclass::Property("maxhamming", ..) => {
                settings.maxhamming = value.get_some().unwrap();
            }
            subclass::Property("decimate", ..) => {
                settings.decimate = value.get_some().unwrap();
            }
            subclass::Property("blur", ..) => {
                settings.blur = value.get_some().unwrap();
            }
            subclass::Property("refine-edges", ..) => {
                settings.refine_edges = value.get_some().unwrap();
            }
            _ => unimplemented!(),
        }

        gst_debug!(CAT, obj: element, "Changing settings to {:?}", settings,);

        let mut state_guard = self.state.lock().unwrap();
        if let Some(state) = state_guard.as_mut() {
            state.inner = make_detector(&settings);
        }
    }

    // Called whenever a value of a property is read. It can be called
    // at any time from any thread.
    fn get_property(&self, _obj: &glib::Object, id: usize) -> Result<glib::Value, ()> {
        let prop = &PROPERTIES[id];

        match *prop {
            subclass::Property("family", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.family.to_value())
            }
            subclass::Property("maxhamming", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.maxhamming.to_value())
            }
            subclass::Property("decimate", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.decimate.to_value())
            }
            subclass::Property("blur", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.blur.to_value())
            }
            subclass::Property("refine-edges", ..) => {
                let settings = self.settings.lock().unwrap();
                Ok(settings.refine_edges.to_value())
            }
            _ => unimplemented!(),
        }
    }

    // Called right after construction of a new instance
    fn constructed(&self, obj: &glib::Object) {
        // Call the parent class' ::constructed() implementation first
        self.parent_constructed(obj);

        // Here we actually add the pads we created in AprilTagDetector::new() to the
        // element so that GStreamer is aware of their existence.
        let element = obj.downcast_ref::<gst::Element>().unwrap();
        element.add_pad(&self.sinkpad).unwrap();
        element.add_pad(&self.srcpad).unwrap();
    }
}

// Implementation of gst::Element virtual methods
impl ElementImpl for AprilTagDetector {
    // Called whenever the state of the element should be changed. This allows for
    // starting up the element, allocating/deallocating resources or shutting down
    // the element again.
    fn change_state(
        &self,
        element: &gst::Element,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst_trace!(CAT, obj: element, "Changing state {:?}", transition);

        // match transition {
        //     gst::StateChange::ReadyToPaused | gst::StateChange::PausedToReady => {
        //         // Reset the whole state
        //         let mut state = self.state.lock().unwrap();
        //         *state = State::default();
        //     }
        //     _ => (),
        // }

        if let gst::StateChange::ReadyToNull = transition {
            *self.state.lock().unwrap() = None;
        }

        // Call the parent class' implementation of ::change_state()
        self.parent_change_state(element, transition)
    }
}

// Registers the type for our element, and then registers in GStreamer under
// the name "apriltagdetector" for being able to instantiate it via e.g.
// gst::ElementFactory::make().
pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "apriltagdetector",
        gst::Rank::None,
        AprilTagDetector::get_type(),
    )
}
