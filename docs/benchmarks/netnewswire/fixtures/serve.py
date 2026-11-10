#!/usr/bin/env python3
"""Serves the benchmark's fixture feeds to the iPhone, from the laptop.

    python3 docs/benchmarks/netnewswire/fixtures/serve.py [--port 8765]

Both apps subscribe to the same addresses, so they show the same content.
Standard library only. Besides the files in this folder:

    /slow.xml      answers rss.xml after 30 seconds (loading states, cancel)
    /missing.xml   404
    /error.xml     500
    /redirect.xml  301 to /rss.xml

Plain files get Last-Modified and answer If-Modified-Since with 304, so a
second refresh shows each app's HTTP caching. Nothing is logged but the path.
"""
import argparse
import http.server
import os
import socket
import time

HERE = os.path.dirname(os.path.abspath(__file__))


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".xml": "application/rss+xml",
        ".json": "application/feed+json",
        ".opml": "text/x-opml",
    }

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/slow.xml":
            time.sleep(30)
            self.path = "/rss.xml"
        elif path == "/missing.xml":
            return self.send_error(404)
        elif path == "/error.xml":
            return self.send_error(500)
        elif path == "/redirect.xml":
            self.send_response(301)
            self.send_header("Location", "/rss.xml")
            self.end_headers()
            return
        elif path.endswith(".py"):
            return self.send_error(404)
        return super().do_GET()

    def log_message(self, fmt, *args):
        print(f"{self.command} {self.path} -> {args[1] if len(args) > 1 else ''}")


def lan_address():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("192.0.2.1", 9))  # no packet is sent
        return s.getsockname()[0]
    except OSError:
        return "127.0.0.1"
    finally:
        s.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("0.0.0.0", args.port), Handler)
    print(f"Fixture feeds at http://{lan_address()}:{args.port}/rss.xml (and atom.xml, feed.json, ...)")
    server.serve_forever()


if __name__ == "__main__":
    main()
