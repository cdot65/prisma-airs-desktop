#!/usr/bin/env python3
"""Synthetic plain-HTTP endpoint for the API Client (C15 native acceptance
and workload measurement). Binds 127.0.0.1 only and records every request
(method, path, selected headers, body size and SHA-256) in
<dir>/requests.jsonl. Routes:

  GET  /items            a small JSON body
  GET  /login            sets a persistent cookie
  GET  /needs-cookie     200 only when that cookie comes back
  GET  /big?bytes=N      N bytes of a repeating pattern, streamed (with
                         &pace=U, U microseconds between 64 KiB writes; the
                         end of the stream is recorded as a second row)
  POST /upload           drained (any size); its size and digest recorded
  ANY  /hang             never answers (until the client goes away)
  POST /lost             closes without any reply (a lost response)
"""
import argparse
import hashlib
import json
import os
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

LOCK = threading.Lock()
KEPT = ("content-type", "cookie", "authorization", "x-trace", "content-length")


def pattern(n):
    return bytes(i % 251 for i in range(n))


CHUNK = pattern(251 * 260)  # a whole number of pattern periods


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def record(self, size, digest):
        entry = {
            "method": self.command,
            "path": self.path,
            "headers": {k: v for k, v in self.headers.items() if k.lower() in KEPT},
            "bytes": size,
            "sha256": digest,
            "at": time.time(),
        }
        with LOCK, open(os.path.join(self.server.dir, "requests.jsonl"), "a") as f:
            f.write(json.dumps(entry) + "\n")

    def drain(self):
        length = int(self.headers.get("content-length") or 0)
        h = hashlib.sha256()
        left = length
        while left > 0:
            piece = self.rfile.read(min(left, 1 << 20))
            if not piece:
                break
            h.update(piece)
            left -= len(piece)
        return length - left, h.hexdigest()

    def answer(self, status, body, headers=()):
        self.send_response(status)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def handle_any(self):
        size, digest = self.drain()
        self.record(size, digest)
        url = urlparse(self.path)
        if url.path == "/hang":
            try:
                self.connection.settimeout(600)
                while self.connection.recv(64):
                    pass
            except OSError:
                pass
            return
        if url.path == "/lost":
            self.connection.shutdown(socket.SHUT_RDWR)
            return
        if url.path == "/items":
            return self.answer(200, b'{"items":[1,2,3]}', [("content-type", "application/json")])
        if url.path == "/login":
            return self.answer(200, b"{}", [("content-type", "application/json"),
                                            ("set-cookie", "sid=synthetic-c15; Path=/; Max-Age=3600")])
        if url.path == "/needs-cookie":
            ok = "sid=synthetic-c15" in (self.headers.get("cookie") or "")
            return self.answer(200 if ok else 401, b'{"cookie":%s}' % (b"true" if ok else b"false"),
                               [("content-type", "application/json")])
        if url.path == "/big":
            query = parse_qs(url.query)
            n = int(query.get("bytes", ["0"])[0])
            pace = int(query.get("pace", ["0"])[0]) / 1_000_000
            self.send_response(200)
            self.send_header("content-type", "application/octet-stream")
            self.send_header("content-length", str(n))
            self.send_header("connection", "close")
            self.end_headers()
            sent = 0
            try:
                while sent < n:
                    piece = CHUNK[: min(len(CHUNK), n - sent)]
                    self.wfile.write(piece)
                    sent += len(piece)
                    if pace:
                        time.sleep(pace)
            except OSError:
                pass
            with LOCK, open(os.path.join(self.server.dir, "requests.jsonl"), "a") as f:
                f.write(json.dumps({"method": "END", "path": url.path, "bytes": sent, "at": time.time()}) + "\n")
            return
        if url.path == "/upload":
            return self.answer(200, json.dumps({"bytes": size, "sha256": digest}).encode(),
                               [("content-type", "application/json")])
        return self.answer(404, b"{}", [("content-type", "application/json")])

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = handle_any


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port, directory):
        self.dir = directory
        super().__init__(("127.0.0.1", port), Handler)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", required=True)
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    os.makedirs(args.dir, exist_ok=True)
    server = Server(args.port, args.dir)
    print(f"ready {server.server_address[1]}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
