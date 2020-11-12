use gstreamer as gst;
use gstreamer_app as gst_app;

use gst::prelude::*;
use futures::stream::Stream;

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("imagesrc-gst::Error")
    }
}

#[derive(Debug)]
enum ErrorKind {
    Glib(glib::Error),
    GlibBool(glib::BoolError),
    GstStateChange(gst::StateChangeError),
    AlreadyGrabbing,
}

impl From<glib::Error> for Error {
    fn from(orig: glib::Error) -> Self {
        Error {kind: ErrorKind::Glib(orig)}
    }
}

impl From<glib::BoolError> for Error {
    fn from(orig: glib::BoolError) -> Self {
        Error {kind: ErrorKind::GlibBool(orig)}
    }
}

impl From<gst::StateChangeError> for Error {
    fn from(orig: gst::StateChangeError) -> Self {
        Error {kind: ErrorKind::GstStateChange(orig)}
    }
}

impl<'a> From<gst::message::Error<'a>> for Error {
    fn from(orig: gst::message::Error<'a>) -> Self {
        Error {kind: ErrorKind::Glib(orig.get_error())}
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error {kind}
    }
}

// ------

pub struct GstSample {
    sample: gst::Sample,
}

impl GstSample {
    pub fn sample(&self) -> &gst::Sample {
        &self.sample
    }
}

impl imagesrc::Sample for GstSample {
}

pub struct GstSink {
    rx: Option<futures::channel::mpsc::Receiver<GstSample>>,
    pub control: thread_control::Control,
    pub join_handle: std::thread::JoinHandle<Result<(),Error>>,
}

impl GstSink {
    pub fn spawn_gstreamer_mainloop(pipeline_elements: Vec<String>, bufsize: usize) -> Self {
        let (mut tx, rx) = futures::channel::mpsc::channel(bufsize);

        let (flag, control) = thread_control::make_pair();

        let join_handle = std::thread::Builder::new()
            .name("gstreamer-mainloop".to_string())
            .spawn(move || {

            gst::init()?;

            let pipeline = gst::Pipeline::new(None);

            let elements: Result<Vec<_>,_> = pipeline_elements
                .iter()
                .map(|name| {
                    let el = gst::ElementFactory::make(name, None);
                    if name=="capsfilter" {
                        println!("Warning: using capsfilter hack {}:{}", file!(), line!());
                        let filter: gst::Element = el.expect("create capsfilter");

                        use std::str::FromStr;
                        let video_caps = gst::Caps::from_str("video/x-raw(memory:NVMM),width=3820,height=2464,framerate=21/1,format=NV12")?;
                        println!("  setting capsfilter caps to {:?}", video_caps);

                        filter.set_property("caps", &video_caps.to_value())?;

                        Ok(filter)
                    } else {
                        el
                    }
                })
                .into_iter()
                .collect();

            let sink = gst::ElementFactory::make("appsink", None)?;

            let mut elements: Vec<_> = elements?;
            elements.push(sink);

            let elements_view = elements.iter().collect::<Vec<_>>();

            pipeline.add_many(&elements_view)?;
            gst::Element::link_many(&elements_view)?;
            let sink = elements.pop().unwrap();

            let appsink = sink
                .dynamic_cast::<gst_app::AppSink>()
                .expect("Sink element is expected to be an appsink!");

            // Tell the appsink what format we want. It will then be the v4l2src's job to
            // provide the format we request.
            // This can be set after linking the two objects, because format negotiation between
            // both elements will happen during pre-rolling of the pipeline.
            appsink.set_caps(Some(&gst::Caps::new_simple(
                "video/x-raw",
                &[
                    ("format", &gstreamer_video::VideoFormat::Gray8.to_str()),
                ],
            )));


            // Getting data out of the appsink is done by setting callbacks on it.
            // The appsink will then call those handlers, as soon as data is available.
            appsink.set_callbacks(
                gst_app::AppSinkCallbacks::new()
                    // Add a handler to the "new-sample" signal.
                    .new_sample(move |appsink| {
                        // Pull the sample in question out of the appsink's buffer.
                        let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        tx.try_send( GstSample{ sample }).expect("try send");
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );

            pipeline.set_state(gst::State::Playing)?;

            let bus = pipeline
                .get_bus()
                .expect("Pipeline without bus. Shouldn't happen!");

            for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
                if !flag.is_alive() {
                    break;
                }
                use gst::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => break,
                    MessageView::Error(err) => {
                        return Err(err.into());
                    }
                    _ => (),
                }
            }

            pipeline.set_state(gst::State::Null)?;
            Ok(())
        }).expect("spawn failed");

        Self {
            rx: Some(rx),
            control,
            join_handle,
        }
    }
}

impl imagesrc::ImageSource<GstSample> for GstSink {
    fn frames(&mut self) -> Result<Box<dyn Stream<Item=GstSample>+Send+Unpin>, Box<dyn std::error::Error>>
    {
        match self.rx.take() {
            Some(rx) => Ok(Box::new(rx)),
            None => Err(Box::new(Error::from(ErrorKind::AlreadyGrabbing))),
        }
    }
}
