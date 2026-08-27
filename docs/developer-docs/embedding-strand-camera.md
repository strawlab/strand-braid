# Embedding Strand Camera in another application

Strand Camera normally runs as its own process with its own browser UI. It can
also run as a library inside another Rust application, on that application's
Tokio runtime. [FLO](https://github.com/strawlab/flo) does this: it hosts one or
two tracking cameras in the same process as its controller, so camera
observations reach the controller over local channels instead of a socket.

This document describes that embedding API. It is not needed to run or build
Strand Camera itself.

## Entry point

```rust
strand_cam::run_strand_cam_app_async_with_host_options(
    camera_module,
    args,
    app_name,
    shutdown_rx,
    Some(host_options),
    Some(embedded_http),
)
.await?;
```

The returned future is deliberately `!Send`: it holds vendor camera objects and
other thread-affine state. Run it on a `LocalSet`, not a work-stealing task.

`shutdown_rx` is a `oneshot` the host fires to stop the camera cleanly.

## `StrandCamHostOptions`

Everything the host wires up for **one** camera. Register one per camera. See
`strand-cam/src/host_options.rs`.

### `cam_args_rx` — camera control, inbound

A bounded channel of `strand_cam_remote_control::CamArg`, routed through the same
command task as `CallbackType::ToCamera` HTTP requests. This is how a host starts
and stops MP4 recording, sets the codec, drives the post-trigger buffer, and so
on, without making an HTTP call to itself.

Closing the channel disables host-side camera control. It does not stop
acquisition.

### `frame_sink` — every acquired frame, outbound

A bounded channel of `HostFrame`, carrying the frame as an `Arc` (no copy, no
conversion), its host-counted frame number, and the most acquisition-faithful
timestamp available, along with which kind of timestamp that is.

**Delivery is lossless.** Strand Camera writes this with `send().await`, so a
host that falls behind applies backpressure to frame processing rather than
silently missing frames. This matters because a host runs its detector here: a
detector that skips frames is a tracker that loses its target.

Two consequences for the host:

- **Drain it promptly, and keep it shallow.** A deep queue only adds tracking
  latency and holds several megabytes per camera. Depth 1–2 is right: enough to
  absorb scheduling jitter, not enough to build a backlog.
- **A host that only wants to display frames must drop them itself.** Not
  reading is not a way to decline a frame; it is a way to slow the camera down.

Losslessness here does not make the whole pipeline lossless. `cam_stream_task`
still drops a frame — loudly, with an error and a UI flag — when the
frame-processing queue fills under sustained overload. The lossless sink moves
the loss out of the detection path and back to that one existing, visible choke
point.

The sink is registered per camera, so `HostFrame` deliberately does not carry a
camera name.

### `annotation_rx` — what the host found, inbound

A `watch` channel of `HostAnnotation`: points the host's own detector found,
drawn on Strand Camera's browser preview. This exists so an embedded camera's BUI
still shows detections after the host takes over detection.

The marks are always retrospective, and cannot be otherwise: the host receives
frame *N* through `frame_sink` and cannot report on it before Strand Camera has
already published frame *N* to the preview. `HostAnnotation` therefore carries
the frame number and timestamp it came from. That provenance reaches the browser
on `ToClient::host_annotation`, and the video field prints it — for example
`detection: frame 1240 (1 frame behind)`.

An annotation with **no** points is meaningful: it is the host saying it looked
and found nothing, which clears the previous mark.

Being a `watch` channel, only the latest value is kept. A host that skips an
update loses a stale mark and nothing else, and a host that stops sending cannot
back anything up.

## `EmbeddedHttpOptions`

A `oneshot` over which Strand Camera hands the host its browser-UI `axum::Router`
once startup has built it. The host mounts it under its own authenticated HTTP
server; the camera does not bind its configured per-camera address. FLO serves
each embedded camera at `/camera/<camera_name>/`.

## What is *not* in this API

Detection. Strand Camera's own ImOps detector serves the standalone deployment,
where it is enabled and tuned from the browser UI and sends moments over UDP. An
embedding host reads `frame_sink` and runs whatever detector it wants, in
whatever thread it wants.

Set `StrandCamArgs::disable_imops` (`--disable-imops` on the command line) when
you do. It leaves `StoreType::im_ops_state` as `None`, which keeps the built-in
detector out of the frame path and its panel out of the browser UI. Both halves
matter:

- Left enabled, it is a second threshold-and-moments pass over every frame,
  paid for nothing — the host is already detecting from `frame_sink`.
- Its panel is worse than wasted. The two enable flags are independent, so the
  panel's checkbox reads *unchecked* while the host's detector is running
  happily. An operator who unchecks it to stop detection sees tracking
  continue; one who checks it starts a detector whose results go to a UDP
  socket the host is not reading.
