#!/usr/bin/env python

import requests
import json
import time
import threading

import BaseHTTPServer

PORT = 8000
DATA_PREFIX = 'data: '

LIVING_MODELS = {}

class BraidProxy:
    def __init__(self):
        self.braid_model_server_url = 'http://127.0.0.1:8397/'
        self.session = requests.session()
        r = self.session.get(self.braid_model_server_url)
        assert(r.status_code == requests.codes.ok)

    def run(self):
        global LIVING_MODELS
        events_url = self.braid_model_server_url + 'events'
        r = self.session.get(events_url,
            stream=True,
            headers={'Accept': 'text/event-stream'},
            )
        for chunk in r.iter_content(chunk_size=None):
            data = parse_chunk(chunk)
            # print('chunk value: %r'%data)

            try:
                update_dict = data['Update']
            except KeyError:
                update_dict = None

            if update_dict is not None:
                obj_id = update_dict['obj_id']
                LIVING_MODELS[obj_id] = update_dict
            else:
                raise NotImplementedError('')

def parse_chunk(chunk):
    lines = chunk.strip().split('\n')
    assert(len(lines)==2)
    assert(lines[0]=='event: braid-pose')
    assert(lines[1].startswith(DATA_PREFIX))
    buf = lines[1][len(DATA_PREFIX):]
    data = json.loads(buf)
    return data

class MyHandler(BaseHTTPServer.BaseHTTPRequestHandler):
    def do_GET(self):
        global LIVING_MODELS

        self.send_response(code=200)
        self.send_header("Content-type", "application/json")
        self.end_headers()

        buf = json.dumps(LIVING_MODELS)
        self.wfile.write(buf)

def main():
    braid_proxy = BraidProxy()
    listen_thread = threading.Thread(target=braid_proxy.run)
    listen_thread.setDaemon(True)
    listen_thread.start()

    handler_class = MyHandler

    httpd = BaseHTTPServer.HTTPServer(("", PORT), handler_class)

    while 1:
        print("serving at port %d"%PORT)
        httpd.serve_forever()

if __name__ == '__main__':
    main()
