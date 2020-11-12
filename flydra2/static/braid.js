
function update_conn_state(ready_state) {
    var connection_state = document.getElementById("conn_state");
    while (connection_state.firstChild) {
        connection_state.removeChild(connection_state.firstChild);
    }
    var buf = ""
    if (ready_state == 0) {
        buf = "connecting"
    }
    if (ready_state == 1) {
        buf = "open"
    }
    if (ready_state == 2) {
        buf = "closed"
    }
    var element = document.createTextNode(buf);
    connection_state.appendChild(element);
}

function update_events(to_listener) {
    var mirror = document.getElementById("current");
    var d = to_listener.msg.Update;
    if (typeof d != "undefined") {
        var latency_in_flydra_msec = to_listener.latency*1000.0;
        var show = {obj_id: d.obj_id, frame: d.frame, x: d.x, y: d.y, z: d.z, latency_in_flydra_msec}
        var buf = JSON.stringify(show);
        var element = document.createElement("pre");
        var content = document.createTextNode(buf);
        while (mirror.firstChild) {
            mirror.removeChild(mirror.firstChild);
        }

        element.appendChild(content);
        mirror.appendChild(element);
    }
}

var SeverEvents = {
    init: function () {

        if (!!window.EventSource) {
            var source = new EventSource("events");
            update_conn_state(source.readyState);

            source.addEventListener('braid', function (e) {
                var to_listener = JSON.parse(e.data);
                update_events(to_listener);
            }, false);

            source.addEventListener('open', function (e) {
                update_conn_state(source.readyState);
            }, false);

            source.addEventListener('error', function (e) {
                update_conn_state(source.readyState);
            }, false);

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
    }
};

function start(){
    SeverEvents.init();
}

start();
