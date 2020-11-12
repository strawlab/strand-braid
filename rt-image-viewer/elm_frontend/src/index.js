require('./roboto.css');
require('./material-icons.css');
require('./mdl/1.3.0/material.css');
require('./main.css');
var live_view_prefix = "#live-view/";

var Elm = require('./Main.elm');
var is_live_preview = document.location.hash.startsWith(live_view_prefix);
var live_preview_image = null;
if (is_live_preview) {
    live_preview_image = document.location.hash.slice(live_view_prefix.length);
}

var app = null;
if (!!window.EventSource) {
    app = Elm.Main.embed(document.getElementById('root'), is_live_preview);
} else {
    var root = document.getElementById("root");
    root.innerHTML = ('<div>'+
        '<h4>EventSource not supported in this browser</h4>'+
        'Read about EventSource (also known as Server-sent events) at <a '+
        'href="https://html.spec.whatwg.org/multipage/'+
        'server-sent-events.html#server-sent-events">whatwg.org</a>.'+
        'See <a href="http://caniuse.com/#feat=eventsource">caniuse.com</a> for '+
        'information about which browsers are supported.'+
        '</div>');
}

var _framerate = 10.0;
var _preview_enabled = true;

var _resume_preview_buf = null;
var _last_frame_render_msec = null;

// This is currently very inefficient because all viewers get all images.

var sever_event_obj = {
    onmessage: function (encoded) {
        var msg = JSON.parse(encoded);

        if (msg.firehose_frame_data_url) {
            if (is_live_preview) {
                this._display_image(msg);
            }
        } else {
            app.ports.event_source_data.send(encoded);
        }

    },

    // in_msg is a AnnotatedFrame
    _display_image: function(in_msg) {
        var msg = {
            name: "firehose_callback",
            // Create a FirehoseCallbackInner type
            args: { fno: in_msg.fno, ts: in_msg.ts, ck: in_msg.ck, name: in_msg.name }
        };
        var echobuf = JSON.stringify(msg);

        if (live_preview_image != in_msg.name) {
            send_message_buf(echobuf);
            return;
        }

        var img = document.getElementById("firehose-img");

        var data_url = in_msg.firehose_frame_data_url;
        img.src = data_url;

        img.onload = function () {
            var text = document.getElementById("firehose-text");
            text.innerHTML = "frame: " + in_msg.fno.toString();

            var now_msec = Date.now();
            var wait_msec = 0;
            if (_framerate < 10.0) {
                var desired_dt_msec = 1.0/_framerate*1000.0;
                var desired_now = _last_frame_render_msec+desired_dt_msec;
                wait_msec = desired_now-now_msec;
                _last_frame_render_msec = now_msec;
            }


            if (!_preview_enabled) {
                _resume_preview_buf = echobuf;
            } else {
                if (wait_msec > 0) {
                    setTimeout(function () { send_message_buf(echobuf); },wait_msec);
                } else {
                    send_message_buf(echobuf);
                }
            }
        }

    }
}

function send_message_buf(buf) {
    var httpRequest = new XMLHttpRequest();
    httpRequest.open('POST', 'callback');
    httpRequest.setRequestHeader("Content-Type", "application/json;charset=UTF-8");
    httpRequest.send(buf);
}

var SeverEvents = {
    init: function (sever_event_obj) {

        if (!!window.EventSource) {

            var event_prefix = "rt-image-events";
            var event_name = null;
            if (live_preview_image) {
                event_name = event_prefix + "/" + live_preview_image;
            } else {
                event_name = event_prefix;
            }
            var source = new EventSource(event_name);

            source.addEventListener('bui_backend', function (e) {
                var encoded = e.data;
                app.ports.event_source_data.send(encoded);
            }, false);

            source.addEventListener('http-video-streaming', function (e) {
                sever_event_obj.onmessage(e.data);
            }, false);

            source.addEventListener('open', function (e) {
                app.ports.ready_state.send(source.readyState);
            }, false);

            source.addEventListener('error', function (e) {
                app.ports.ready_state.send(source.readyState);
            }, false);

        } else {
            console.error("no EventSource. failing.");
        }
    }
};

function start() {
    app.ports.show_live_view.subscribe(function(name) {
        var strWindowFeatures = "resizable=yes,scrollbars=yes";
        var windowName = "RtImage_Live_Preview/" + name;
        var url = live_view_prefix + name;
        var windowRef = window.open(url, windowName, strWindowFeatures);
    })
    app.ports.set_max_framerate.subscribe(function (max_framerate) {
        _framerate = max_framerate;
    });

    SeverEvents.init(sever_event_obj);
}

start();
