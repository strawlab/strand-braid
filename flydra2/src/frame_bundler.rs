use std::{cmp::Ordering, pin::Pin};

use futures::{
    stream::Stream,
    task::{Context, Poll},
};
use pin_project::pin_project;

use crate::FrameDataAndPoints;

use crate::bundled_data::BundledAllCamsOneFrameDistorted;
use crate::connected_camera_manager::HasCameraList;

/// Orders data from all available cameras from a given frame.
///
/// The returned stream will be monotonically increasing. Note that out-of-order
/// data will be dropped and, although the returned values will be monotonically
/// increasing, it will not, in general, be contiguous. In otherwords, it is
/// possible that there will be gaps in the resulting monotonically increasing
/// sequence.
#[pin_project]
pub(crate) struct OrderedLossyFrameBundler<St, HCL>
where
    St: Stream<Item = StreamItem>,
    HCL: HasCameraList,
{
    #[pin]
    stream: St,
    ccm: HCL,
    current: Option<BundledAllCamsOneFrameDistorted>,
    #[pin]
    pending: Option<StreamItem>,
}

pub enum StreamItem {
    EOF,
    Packet(FrameDataAndPoints),
}

impl<St, HCL> OrderedLossyFrameBundler<St, HCL>
where
    St: Stream<Item = StreamItem>,
    HCL: HasCameraList,
{
    fn new(stream: St, ccm: HCL) -> Self {
        Self {
            stream,
            ccm,
            current: None,
            pending: None,
        }
    }
}

impl<St, HCL> Stream for OrderedLossyFrameBundler<St, HCL>
where
    St: Stream<Item = StreamItem>,
    HCL: HasCameraList,
{
    // In theory, it would be possible to return contiguous frame values,
    // but this would require more complexity here.
    type Item = BundledAllCamsOneFrameDistorted;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<BundledAllCamsOneFrameDistorted>> {
        use futures::ready;

        loop {
            let all_cameras = self.ccm.camera_list();

            let mut this = self.as_mut().project();

            // ensure that we have a pending item to work with, return if not.
            if this.pending.is_none() {
                let item = match ready!(this.stream.poll_next(cx)) {
                    Some(e) => e,
                    None => return Poll::Ready(None),
                };
                this.pending.set(Some(item));
            }

            // The following unwrap cannot fail because of above.
            let new_item: FrameDataAndPoints = match this.pending.take().unwrap() {
                StreamItem::EOF => return Poll::Ready(this.current.take()),
                StreamItem::Packet(new_item) => new_item,
            };

            if this.current.is_none() {
                // In the case of no existing data, save this new frame data.
                *this.current = Some(BundledAllCamsOneFrameDistorted::new(new_item));
            } else {
                // In the case of existing data, check if we are done or not.
                let dt = {
                    let current = this.current.as_ref().unwrap();
                    new_item.frame_data.synced_frame.0 as i64 - current.frame().0 as i64
                };

                match dt.cmp(&0) {
                    Ordering::Equal => {
                        // new packet from ongoing frame.
                        let current: &mut Option<BundledAllCamsOneFrameDistorted> = this.current;
                        let current_cameras = {
                            let x = current.as_mut().unwrap();
                            x.push(new_item);
                            x.cameras()
                        };

                        if current_cameras == &all_cameras {
                            let previous = current.take().unwrap();
                            *current = None;
                            return Poll::Ready(Some(previous));
                        }
                    }
                    Ordering::Greater => {
                        // New packet from future frame. Return the accumulated-
                        // until now data and start accumulating from this new
                        // data.
                        let current: &mut Option<BundledAllCamsOneFrameDistorted> = this.current;
                        let previous = current.take().unwrap();
                        *current = Some(BundledAllCamsOneFrameDistorted::new(new_item));
                        return Poll::Ready(Some(previous));
                    }
                    Ordering::Less => {
                        // Drop `new_item` because it has higher latency.
                    }
                }
            }
        }
    }
}

pub(crate) fn bundle_frames<St, HCL>(stream: St, ccm: HCL) -> OrderedLossyFrameBundler<St, HCL>
where
    St: Stream<Item = StreamItem>,
    HCL: HasCameraList,
{
    OrderedLossyFrameBundler::new(stream, ccm)
}

#[test]
fn test_frame_bundler() {
    use futures::stream::{self, StreamExt};

    use crate::{FlydraFloatTimestampLocal, FrameData, SyncFno};

    let cam_name_1 = crate::RosCamName::new("cam1".into());
    let cam_num_1 = crate::CamNum(1);
    let cam_name_2 = crate::RosCamName::new("cam2".into());
    let cam_num_2 = crate::CamNum(2);
    let trigger_timestamp = None;

    let packet1_frame1_cam1 = FrameDataAndPoints {
        frame_data: FrameData::new(
            cam_name_1,
            cam_num_1,
            SyncFno(1),
            trigger_timestamp.clone(),
            FlydraFloatTimestampLocal::from_f64(0.0),
            None,
            None,
        ),
        points: Vec::new(),
    };

    let packet2_frame1_cam2 = FrameDataAndPoints {
        frame_data: FrameData::new(
            cam_name_2.clone(),
            cam_num_2,
            SyncFno(1),
            trigger_timestamp.clone(),
            FlydraFloatTimestampLocal::from_f64(0.0),
            None,
            None,
        ),
        points: Vec::new(),
    };

    let packet2_frame0_cam2 = FrameDataAndPoints {
        frame_data: FrameData::new(
            cam_name_2.clone(),
            cam_num_2,
            SyncFno(0),
            trigger_timestamp.clone(),
            FlydraFloatTimestampLocal::from_f64(0.0),
            None,
            None,
        ),
        points: Vec::new(),
    };

    let packet2_frame2_cam2 = FrameDataAndPoints {
        frame_data: FrameData::new(
            cam_name_2.clone(),
            cam_num_2,
            SyncFno(2),
            trigger_timestamp.clone(),
            FlydraFloatTimestampLocal::from_f64(0.0),
            None,
            None,
        ),
        points: Vec::new(),
    };

    let packet2_frame3_cam2 = FrameDataAndPoints {
        frame_data: FrameData::new(
            cam_name_2,
            cam_num_2,
            SyncFno(3),
            trigger_timestamp,
            FlydraFloatTimestampLocal::from_f64(0.0),
            None,
            None,
        ),
        points: Vec::new(),
    };

    // with zero packets

    let inputs: Vec<_> = vec![StreamItem::EOF];

    let cameras = crate::connected_camera_manager::CameraList::new(&[1, 2]);
    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 0);

    // with one packet

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::EOF,
    ];

    let expected = packet1_frame1_cam1.clone();
    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 1);
    assert_eq!(
        actual[0],
        BundledAllCamsOneFrameDistorted::new(expected.clone())
    );

    // with two packets from same frame

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::Packet(packet2_frame1_cam2.clone()),
        StreamItem::EOF,
    ];

    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].num_cameras(), 2);

    // with two packets from with a later outdated frame

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::Packet(packet2_frame0_cam2),
        StreamItem::EOF,
    ];

    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0], BundledAllCamsOneFrameDistorted::new(expected));

    // with two subsequent packets

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::Packet(packet2_frame2_cam2),
        StreamItem::EOF,
    ];

    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].num_cameras(), 1);
    assert_eq!(actual[1].num_cameras(), 1);

    // with non-adjacent subsequent packets

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::Packet(packet2_frame3_cam2),
        StreamItem::EOF,
    ];

    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].num_cameras(), 1);
    assert_eq!(actual[1].num_cameras(), 1);

    // At the moment all frames arrived and not one frame later. Thus, no EOF
    // marker.

    let inputs: Vec<_> = vec![
        StreamItem::Packet(packet1_frame1_cam1.clone()),
        StreamItem::Packet(packet2_frame1_cam2),
    ];

    let bundled = bundle_frames(stream::iter(inputs), cameras.clone());
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].num_cameras(), 2);

    // But not if only one frame arrives.

    let inputs: Vec<_> = vec![StreamItem::Packet(packet1_frame1_cam1)];

    let bundled = bundle_frames(stream::iter(inputs), cameras);
    let actual: Vec<_> = futures::executor::block_on(bundled.collect());
    assert_eq!(actual.len(), 0);
}

#[test]
fn test_async_stream_ops() {
    use futures::future;
    use futures::stream::{self, StreamExt};

    let stream = stream::iter(1..=10);
    let evens = stream.filter_map(|x| {
        let ret = if x % 2 == 0 { Some(x + 1) } else { None };
        future::ready(ret)
    });

    let result: Vec<_> = futures::executor::block_on(evens.collect());
    assert_eq!(vec![3, 5, 7, 9, 11], result);
}
