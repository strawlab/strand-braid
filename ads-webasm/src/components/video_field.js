export function set_frame_load_callback(data_url, in_msg2, jscallback) {
    let img = new Image();
    img.src = data_url;
    img.onload = function () {
        let handle = {
            img,
            in_msg2,
        };

        jscallback(handle);
        // jscallback.drop();
    };
}

export function do_frame_loaded(max_framerate, css_id, last_frame_render_msec, handle) {

    // TODO:
    // fix DRY with send_message_buf() by using send_message()
    // in main?
    function send_message_buf(buf) {
        var httpRequest = new XMLHttpRequest();
        httpRequest.open("POST", "callback");
        httpRequest.setRequestHeader("Content-Type", "application/json;charset=UTF-8");
        httpRequest.send(buf);
    }

    let img = handle.img;

    let canvas = document.getElementById(css_id);
    let ctx = canvas.getContext("2d");
    ctx.drawImage(img, 0, 0);

    let in_msg = handle.in_msg2;

    ctx.strokeStyle = "#7FFF7f";
    ctx.lineWidth = 1.0;

    in_msg.found_points.forEach(function (pt) {
        ctx.beginPath();
        ctx.arc(pt.x, pt.y, 30.0, 0, Math.PI * 2, true); // circle
        var r = 30.0;
        if (pt.theta) {
            var dx = r * Math.cos(pt.theta);
            var dy = r * Math.sin(pt.theta);
            ctx.moveTo(pt.x - dx, pt.y - dy);
            ctx.lineTo(pt.x + dx, pt.y + dy);
        }
        ctx.closePath();
        ctx.stroke();
    });

    in_msg.draw_shapes.forEach(function (drawable_shape) {
        ctx.strokeStyle = drawable_shape.stroke_style;
        ctx.lineWidth = drawable_shape.line_width;
        // shape will have either "Circle", "Polygon", or "Everything"
        var circle = drawable_shape.shape["Circle"];
        if (typeof circle != "undefined") {
            ctx.beginPath();
            ctx.arc(circle.center_x, circle.center_y, circle.radius, 0, Math.PI * 2, true); // circle
            ctx.closePath();
            ctx.stroke();
        }

        var polygon = drawable_shape.shape["Polygon"];
        if (typeof polygon != "undefined") {
            var p = polygon.points;
            ctx.beginPath();
            ctx.moveTo(p[0].x, p[1].y);
            for (i = 1; i < p.length; i++) {
                ctx.lineTo(p[i].x, p[i].y);
            }
            ctx.closePath();
            ctx.stroke();
        }

    });

    let now_msec = Date.now();
    let wait_msec = 0;
    let desired_dt_msec = 1.0 / max_framerate * 1000.0;
    let desired_now = last_frame_render_msec + desired_dt_msec;
    wait_msec = desired_now - now_msec;
    last_frame_render_msec = now_msec;

    // Create a FirehoseCallbackInner type
    let echobuf = JSON.stringify({ FirehoseNotify: in_msg });

    if (wait_msec > 0) {
        // TODO FIXME XXX: eliminate "window." access to global variable
        if (!window.is_sleeping) {
            window.is_sleeping = true;
            setTimeout(function () {
                window.is_sleeping = false;
                send_message_buf(echobuf);
            }, wait_msec);
        }
    } else {
        send_message_buf(echobuf);
    }

    return last_frame_render_msec;
}
