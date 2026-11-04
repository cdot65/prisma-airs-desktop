#!/usr/bin/env python3
"""Synthetic Runtime Security scan endpoint for C03 native acceptance and
performance runs. It serves HTTPS on localhost with a certificate from a
disposable test CA (generated with openssl into a scratch directory), answers
POST /v1/scan/sync/request according to a mode file read on every request,
and appends one JSON line per request it receives. It never contacts the
network and only ever sees synthetic keys.

  scan_server.py --dir <scratch> --port <port> --key <synthetic key>

Files in <scratch>: ca.pem (the root the fixture app trusts), mode.json
({"reply": "allow"|"block"|"unknownAction"|"incomplete"|"contentError"|"401"|"close",
"delayMs": n}) and requests.jsonl.
"""

import argparse
import hashlib
import hmac
import json
import os
import ssl
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PATH = "/v1/scan/sync/request"


def make_certs(directory):
    """A test CA and a localhost leaf (P-256) signed by it."""
    os.makedirs(directory, exist_ok=True)
    run = lambda *argv: subprocess.run(argv, check=True, capture_output=True, cwd=directory)
    run("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "ca.key")
    # Explicit SHA-256: the system LibreSSL otherwise signs with SHA-1, which
    # macOS trust evaluation rejects as a broken signature algorithm.
    run("openssl", "req", "-x509", "-new", "-sha256", "-key", "ca.key", "-days", "2", "-subj", "/CN=airs-v2 c03 fixture ca",
        "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-out", "ca.pem")
    run("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "leaf.key")
    run("openssl", "req", "-new", "-key", "leaf.key", "-subj", "/CN=localhost", "-out", "leaf.csr")
    with open(os.path.join(directory, "leaf.ext"), "w") as f:
        f.write("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n"
                "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n")
    run("openssl", "x509", "-req", "-sha256", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
        "-days", "2", "-extfile", "leaf.ext", "-out", "leaf.pem")


def result_body(mode):
    body = {
        "report_id": "R-c03-native", "scan_id": "S-c03-native", "tr_id": "T-c03",
        "profile_name": "Production guardrails", "category": "benign", "action": "allow",
        "timeout": False, "error": False, "errors": [],
        "prompt_detected": {"injection": False, "dlp": False}, "response_detected": {},
    }
    if mode == "block":
        body.update(category="malicious", action="block", prompt_detected={"injection": True, "dlp": False, "future_detector": True})
    elif mode == "unknownAction":
        body.update(action="quarantine-v2")
    elif mode == "incomplete":
        body.update(timeout=True)
    elif mode == "contentError":
        body.update(errors=[{"content_type": "prompt", "feature": "dlp", "status": "error"}])
    return json.dumps(body).encode()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "airs-v2-c03-fixture"

    def log_message(self, *_):
        pass

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length)
        key = self.server.key.encode()
        token = self.headers.get("x-pan-token") or ""
        record = {
            "t": time.time(),
            "method": "POST",
            "path": self.path,
            "contentType": self.headers.get("content-type"),
            # Synthetic key: compared, never echoed in full.
            "tokenMatches": hmac.compare_digest(token.encode(), key),
            "payloadHashMatches": hmac.compare_digest(
                (self.headers.get("x-payload-hash") or "").encode(),
                hmac.new(key, body, hashlib.sha256).hexdigest().encode()),
            "authorization": self.headers.get("authorization") is not None,
            "bodyBytes": len(body),
        }
        try:
            parsed = json.loads(body)
            contents = parsed.get("contents") or [{}]
            record["profile"] = parsed.get("ai_profile")
            record["metadata"] = parsed.get("metadata")
            record["promptChars"] = len(contents[0].get("prompt") or "")
            record["responseChars"] = len(contents[0].get("response") or "")
            record["promptHead"] = (contents[0].get("prompt") or "")[:64]
        except (ValueError, AttributeError, IndexError):
            record["unparsed"] = True
        mode = self.server.mode()
        record["reply"] = mode.get("reply", "allow")
        with self.server.lock, open(self.server.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        time.sleep(mode.get("delayMs", 0) / 1000)
        reply = record["reply"]
        if self.path != PATH:
            self.respond(404, b"{}")
        elif reply == "close":
            self.close_connection = True
            self.connection.close()
        elif reply == "401":
            self.respond(401, b'{"error":{"message":"synthetic denial"}}')
        else:
            self.respond(200, result_body(reply))

    def respond(self, status, payload):
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)
        self.close_connection = True


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port, directory, key):
        super().__init__(("127.0.0.1", port), Handler)
        self.directory, self.key = directory, key
        self.log = os.path.join(directory, "requests.jsonl")
        self.lock = threading.Lock()

    def mode(self):
        try:
            with open(os.path.join(self.directory, "mode.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {"reply": "allow"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--key", required=True)
    parser.add_argument("--make-certs", action="store_true")
    args = parser.parse_args()
    if args.make_certs:
        make_certs(args.dir)
        return 0
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(os.path.join(args.dir, "leaf.pem"), os.path.join(args.dir, "leaf.key"))
    server = Server(args.port, args.dir, args.key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    print("ready", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
