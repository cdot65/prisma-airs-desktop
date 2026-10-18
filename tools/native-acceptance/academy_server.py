#!/usr/bin/env python3
"""Synthetic Academy + Clerk endpoint for C04/C05 native acceptance: the
public desktop configuration, RFC 8414 metadata, device authorization,
token, revocation, the learner `desktop.me` procedure and (C05) the learner
catalog procedures from <scratch>/catalog.json (academy_catalog.build), over
HTTPS on localhost with a disposable test CA (certificates from
scan_server.make_certs). It never contacts the network and issues only
synthetic tokens.

  academy_server.py --dir <scratch> --port <port>

<scratch>/mode.json: {"approval": "pending"|"granted"|"denied",
"displayName": "..."}; <scratch>/requests.jsonl records each request's
method, path, form field names, client_id, grant type and whether an
Authorization header was present (token values are never recorded); learner
procedure requests also record the procedure, the SuperJSON input and the
endpoint's own handling time in milliseconds.
"""

import argparse
import json
import os
import ssl
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SCOPE = "openid profile email offline_access academy:learn"
CLIENT = "desktop_public_client"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "airs-v2-c04-fixture"

    def log_message(self, *_):
        pass

    def reply(self, status, body):
        payload = json.dumps(body).encode()
        self.handled_ms = (time.perf_counter() - self.started) * 1000
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)
        self.close_connection = True

    def record(self, form, extra=None):
        entry = {"t": time.time(), "method": self.command, "path": self.path.split("?")[0],
                 "fields": sorted(form), "clientId": form.get("client_id"),
                 "grantType": form.get("grant_type"), "tokenHint": form.get("token_type_hint"),
                 "revokedToken": form.get("token") if self.path == "/oauth/revoke" else None,
                 "authorization": self.headers.get("authorization") is not None,
                 "clientSecret": "client_secret" in form, **(extra or {})}
        with self.server.lock, open(os.path.join(self.server.dir, "requests.jsonl"), "a") as f:
            f.write(json.dumps(entry) + "\n")

    def do_GET(self):
        self.started = time.perf_counter()
        query = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(self.path).query))
        if self.path.startswith("/api/trpc/"):
            return self.learner_call(query.get("input"))
        self.handle_request({})

    def do_POST(self):
        self.started = time.perf_counter()
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length).decode()
        if self.path.startswith("/api/trpc/") and not self.path.startswith("/api/trpc/desktop.me"):
            return self.learner_call(body or None)
        form = dict(urllib.parse.parse_qsl(body))
        self.handle_request(form)

    def learner_call(self, raw_input):
        """A catalog procedure: the reply for its input, else NOT_FOUND."""
        procedure = self.path.split("?")[0][len("/api/trpc/"):]
        if procedure == "desktop.me":
            return self.handle_request({})
        entry = self.server.catalog().get(procedure, KeyError)
        try:
            value = (json.loads(raw_input).get("json") if raw_input else None) or {}
        except ValueError:
            value = {}
        if isinstance(entry, dict) and set(entry) == {"by", "values"}:
            entry = entry["values"].get(str(value.get(entry["by"])), KeyError) if isinstance(value, dict) else KeyError
        # Access as Academy decides it: guest (no credential) or account.
        if isinstance(entry, dict) and set(entry) == {"__auth__"}:
            entry = entry["__auth__"]["account" if self.headers.get("authorization") else "guest"]
        if isinstance(entry, dict) and set(entry) == {"__error__"}:
            err = entry["__error__"]
            self.reply(err["status"], {"error": {"json": {"message": err["message"], "code": err["rpc"],
                                                          "data": {"code": err["code"], "httpStatus": err["status"],
                                                                   "path": procedure, **err.get("data", {})}}}})
            self.record({}, {"procedure": procedure, "input": raw_input, "status": err["status"],
                             "handlingMs": round(self.handled_ms, 3)})
            return
        if entry is KeyError:
            self.reply(404, {"error": {"json": {"message": "No such procedure or item", "code": -32004,
                                                "data": {"code": "NOT_FOUND", "httpStatus": 404, "path": procedure}}}})
        else:
            self.reply(200, {"result": {"data": {"json": entry}}})
        self.record({}, {"procedure": procedure, "input": raw_input, "status": 404 if entry is KeyError else 200,
                         "handlingMs": round(self.handled_ms, 3)})

    def handle_request(self, form):
        self.record(form)
        origin = self.server.origin
        mode = self.server.mode()
        path = self.path.split("?")[0]
        if path == "/api/desktop/config":
            return self.reply(200, {"issuer": origin, "clientId": CLIENT, "scope": SCOPE})
        if path == "/.well-known/oauth-authorization-server":
            return self.reply(200, {"issuer": origin, "device_authorization_endpoint": f"{origin}/oauth/device",
                                    "token_endpoint": f"{origin}/oauth/token",
                                    "revocation_endpoint": f"{origin}/oauth/revoke"})
        if path == "/oauth/device":
            return self.reply(200, {"device_code": "fixture-device-code", "user_code": "WDJB-MJHT",
                                    "verification_uri": f"{origin}/activate",
                                    "verification_uri_complete": f"{origin}/activate?user_code=WDJB-MJHT",
                                    "expires_in": 600, "interval": 1})
        if path == "/oauth/token":
            if form.get("grant_type") == "refresh_token":
                return self.reply(200, {"token_type": "Bearer", "access_token": "fixture-access-2",
                                        "refresh_token": form.get("refresh_token"), "expires_in": 3600, "scope": SCOPE})
            approval = mode.get("approval", "pending")
            if approval == "granted":
                return self.reply(200, {"token_type": "Bearer", "access_token": "fixture-access-1",
                                        "refresh_token": "fixture-refresh-1", "expires_in": 3600, "scope": SCOPE})
            if approval == "denied":
                return self.reply(400, {"error": "access_denied"})
            return self.reply(400, {"error": "authorization_pending"})
        if path == "/oauth/revoke":
            return self.reply(200, {})
        if path == "/api/trpc/desktop.me":
            return self.reply(200, {"result": {"data": {"json": {"accountId": "user_2fixturelearner",
                                    "displayName": mode.get("displayName", "Ada Lovelace"),
                                    "serverTime": "2026-10-17T00:00:00.000Z"}}}})
        return self.reply(404, {})


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port, directory):
        super().__init__(("127.0.0.1", port), Handler)
        self.dir = directory
        self.origin = f"https://localhost:{port}"
        self.lock = threading.Lock()
        self._catalog = None

    def catalog(self):
        if self._catalog is None:
            try:
                with open(os.path.join(self.dir, "catalog.json")) as f:
                    self._catalog = json.load(f)
            except (OSError, ValueError):
                self._catalog = {}
        return self._catalog

    def mode(self):
        try:
            with open(os.path.join(self.dir, "mode.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", required=True)
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(os.path.join(args.dir, "leaf.pem"), os.path.join(args.dir, "leaf.key"))
    server = Server(args.port, args.dir)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    print("ready", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
