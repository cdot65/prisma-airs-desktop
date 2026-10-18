#!/usr/bin/env python3
"""Synthetic Academy + Clerk endpoint for C04-C06 native acceptance: the
public desktop configuration, RFC 8414 metadata, device authorization,
token, revocation, the learner `desktop.me` procedure and (C05/C06) the
learner procedures from <scratch>/catalog.json (academy_catalog.build,
academy_program.build), over
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

Catalog replies may be wrapped: {"by": field, "values": {...}} picks by an
input field; {"__seq__": [...]} answers successive calls in turn (the last
repeats); {"__delay__": {"ms": n, "reply": ...}} answers late (a slow
optional read); {"__timed__": {"reply": ..., "field": f, "in_s": n}} sets
reply[f] to an ISO time n seconds after the request (mode.json "timed":
{procedure: n} overrides n); {"__auth__": {"guest": ..., "account": ...}}
follows Academy's access decision; {"__error__": {...}} is a tRPC error.

With "__avatar__" in the catalog the endpoint keeps one learner avatar the
way Academy does: settings.avatarUploadUrl presigns a PUT to
/upload/avatars/<file>?X-Amz-Signature=fixture on this origin, PUT stores the
bytes (recording content type, length and whether Authorization was sent),
settings.confirmAvatar records <origin>/api/avatars/<file>?v=<n> only after a
stored upload (else NOT_FOUND), settings.removeAvatar clears it,
settings.get reports it, and GET /api/avatars/<file> serves it publicly.
mode.json "lose_put_reply" stores a PUT's bytes but sends no reply.
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
        if self.path.startswith("/api/avatars/") and self.server.catalog().get("__avatar__"):
            path = self.path.split("?")[0]
            self.serve_avatar(path)
            return self.record({}, {"avatarRead": path, "status": 200 if self.server.avatar else 404})
        self.handle_request({})

    def do_PUT(self):
        self.started = time.perf_counter()
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length)
        path = self.path.split("?")[0]
        stored = path.startswith("/upload/avatars/") and "X-Amz-Signature=" in self.path
        if stored:
            with self.server.lock:
                self.server.uploads[path[len("/upload/avatars/"):]] = (self.headers.get("content-type"), body)
        if stored and self.server.mode().get("lose_put_reply"):
            # The store keeps the bytes but its reply never arrives.
            self.close_connection = True
            self.handled_ms = (time.perf_counter() - self.started) * 1000
            return self.record({}, {"put": True, "contentType": self.headers.get("content-type"),
                                    "bytes": len(body), "status": 0})
        self.reply(200 if stored else 403, {})
        self.record({}, {"put": True, "contentType": self.headers.get("content-type"), "bytes": len(body),
                         "status": 200 if stored else 403})

    def serve_avatar(self, path):
        file = path[len("/api/avatars/"):]
        current = self.server.avatar
        if not current or current[0] != file:
            return self.reply(404, {})
        content_type, body = self.server.uploads.get(file, (None, b""))
        self.handled_ms = (time.perf_counter() - self.started) * 1000
        self.send_response(200)
        self.send_header("content-type", content_type or "application/octet-stream")
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def avatar_call(self, procedure, value):
        """Academy's avatar procedures over the endpoint's single avatar."""
        origin, user = self.server.origin, "user_2fixturelearner"
        catalog = self.server.catalog()
        settings = json.loads(json.dumps(catalog.get("settings.get") or {}))
        if procedure == "settings.avatarUploadUrl":
            ext = {"image/jpeg": "jpg", "image/png": "png", "image/webp": "webp"}.get(value.get("contentType"))
            if not ext or not isinstance(value.get("size"), int) or value["size"] > 2 * 1024 * 1024:
                return None, (400, "BAD_REQUEST", -32600, "Unsupported avatar")
            return {"uploadUrl": f"{origin}/upload/avatars/{user}.{ext}?X-Amz-Signature=fixture",
                    "key": f"avatars/{user}.{ext}"}, None
        if procedure == "settings.confirmAvatar":
            file = f"{user}.{value.get('ext')}"
            if file not in self.server.uploads:
                return None, (404, "NOT_FOUND", -32004, "avatar upload not found — try uploading again")
            self.server.version += 1
            self.server.avatar = (file, self.server.version)
        if procedure == "settings.removeAvatar":
            self.server.avatar = None
        if self.server.avatar:
            file, version = self.server.avatar
            settings.setdefault("account", {})["avatarUrl"] = f"{origin}/api/avatars/{file}?v={version}"
        return settings, None

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
        if self.server.catalog().get("__avatar__") and procedure in (
                "settings.get", "settings.avatarUploadUrl", "settings.confirmAvatar", "settings.removeAvatar"):
            with self.server.lock:
                reply, failure = self.avatar_call(procedure, value if isinstance(value, dict) else {})
            entry = reply if failure is None else {"__error__": {"status": failure[0], "code": failure[1],
                                                                  "rpc": failure[2], "message": failure[3]}}
        if isinstance(entry, dict) and set(entry) == {"by", "values"}:
            entry = entry["values"].get(str(value.get(entry["by"])), KeyError) if isinstance(value, dict) else KeyError
        if isinstance(entry, dict) and set(entry) == {"__seq__"}:
            with self.server.lock:
                n = self.server.calls.get(procedure, 0)
                self.server.calls[procedure] = n + 1
            entry = entry["__seq__"][min(n, len(entry["__seq__"]) - 1)]
        if isinstance(entry, dict) and set(entry) == {"__delay__"}:
            time.sleep(entry["__delay__"]["ms"] / 1000)
            entry = entry["__delay__"]["reply"]
        if isinstance(entry, dict) and set(entry) == {"__timed__"}:
            timed = entry["__timed__"]
            seconds = (self.server.mode().get("timed") or {}).get(procedure, timed["in_s"])
            entry = {**timed["reply"], timed["field"]: time.strftime(
                "%Y-%m-%dT%H:%M:%S.000Z", time.gmtime(time.time() + seconds))}
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
        self.calls = {}
        self.uploads = {}
        self.avatar = None
        self.version = 0
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
