#!/usr/bin/env python3
"""Synthetic Prisma AIRS management endpoint for C08 native acceptance. It
serves HTTPS on localhost with the scan endpoint's disposable test CA
(scan_server.py --make-certs), issues synthetic OAuth tokens at
/oauth2/access_token, answers the AIRS management and DLP reads the desktop
makes under /aisec and /dlp according to a mode file read on every request,
and appends one JSON line per request. It never contacts the network and
only ever sees synthetic credentials; secrets are compared, never logged.

  mgmt_server.py --certs <dir with leaf.pem/leaf.key> --dir <scratch> --port <port>
                 --client-id <synthetic id> --secret <synthetic secret> --tsg <digits>

Files in <scratch>: mode.json ({"grant": "ok"|"deny", "profiles": "ok"|"deny",
"topics": "ok"|"deny"|"empty", "profileCount": n, "delayMs": {"<path prefix>": ms}})
and requests.jsonl.
"""

import argparse
import base64
import hmac
import json
import os
import ssl
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, urlsplit

TOKEN_PREFIX = "c08-mgmt-access-"


def profile(n):
    return {"profile_id": f"00000000-0000-4000-8000-{n:012d}", "profile_name": f"Profile {n:02d}", "revision": 1, "active": True}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "airs-v2-c08-fixture"

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.handle_request("GET")

    def do_POST(self):
        self.handle_request("POST")

    def authorization(self):
        value = self.headers.get("authorization") or ""
        srv = self.server
        if value.startswith("Basic "):
            # RFC 6749 2.3.1: both parts form-urlencoded before encoding.
            expected = base64.b64encode(f"{quote(srv.client_id, safe='-._~')}:{quote(srv.secret, safe='-._~')}".encode()).decode()
            return "basic-ok" if hmac.compare_digest(value[6:].encode(), expected.encode()) else "basic-bad"
        if value.startswith("Bearer "):
            token = value[7:]
            with srv.lock:
                issued = token in srv.issued
            return "bearer-issued" if issued else "bearer-other"
        return "none" if not value else "other"

    def handle_request(self, method):
        srv = self.server
        url = urlsplit(self.path)
        query = {k: v[0] for k, v in parse_qs(url.query).items()}
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length) if length else b""
        mode = srv.mode()
        record = {
            "t": time.time(),
            "method": method,
            "path": url.path,
            "query": query,
            "auth": self.authorization(),
            "tsgHeader": self.headers.get("x-pan-tsg-id") or self.headers.get("tsg_id"),
            "scanToken": self.headers.get("x-pan-token") is not None,
        }
        if url.path == "/oauth2/access_token":
            form = {k: v[0] for k, v in parse_qs(body.decode("utf-8", "replace")).items()}
            record["grantType"] = form.get("grant_type")
            record["scope"] = form.get("scope")
            # The secret never appears in the form body (client auth is Basic).
            record["secretInBody"] = srv.secret in body.decode("utf-8", "replace")
        for prefix, ms in (mode.get("delayMs") or {}).items():
            if url.path.startswith(prefix):
                time.sleep(ms / 1000)
        status, payload = self.reply(url.path, query, record, mode)
        record["status"] = status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        self.respond(status, json.dumps(payload).encode())

    def reply(self, path, query, record, mode):
        srv = self.server
        tsg = srv.tsg
        if path == "/oauth2/access_token":
            if mode.get("grant", "ok") != "ok" or record["auth"] != "basic-ok":
                return 401, {"error": "invalid_client", "error_description": "SERVER-TEXT-NEVER-SHOWN"}
            with srv.lock:
                srv.counter += 1
                token = f"{TOKEN_PREFIX}{srv.counter}"
                srv.issued.add(token)
            return 200, {"access_token": token, "token_type": "Bearer", "expires_in": 3600}
        if record["auth"] != "bearer-issued":
            return 401, {"error": "unauthorized"}
        offset = int(query.get("offset", 0))
        limit = int(query.get("limit", 100))
        if path == f"/aisec/v1/mgmt/profiles/tsg/{tsg}":
            if mode.get("profiles", "ok") == "deny":
                return 403, {"error": "forbidden", "message": "SERVER-TEXT-NEVER-SHOWN"}
            count = int(mode.get("profileCount", 30))
            end = min(offset + limit, count)
            body = {"ai_profiles": [profile(n) for n in range(offset, end)]}
            if end < count:
                body["next_offset"] = end
            return 200, body
        if path == f"/aisec/v1/mgmt/topics/tsg/{tsg}":
            topics = mode.get("topics", "ok")
            if topics == "deny":
                return 403, {"error": "forbidden", "message": "SERVER-TEXT-NEVER-SHOWN"}
            if topics == "empty":
                return 200, {"custom_topics": []}
            return 200, {"custom_topics": [{"topic_id": "11111111-1111-4111-8111-111111111111", "topic_name": "Finance",
                                            "revision": 1, "description": "Money talk"}]}
        if path == f"/aisec/v1/mgmt/customerapp/tsg/{tsg}":
            return 200, {"customer_apps": [{"customer_appId": "app-1", "tsg_id": tsg, "app_name": "Support bot",
                                            "cloud_provider": "aws", "environment": "production"}]}
        if path == f"/aisec/v1/mgmt/apikeys/tsg/{tsg}":
            return 200, {"api_keys": [{"api_key_id": "key-1", "api_key_name": "Runtime", "api_key_last8": "abcd1234",
                                       "api_key": "SYNTHETIC-C08-KEY-VALUE", "auth_code": "SYNTHETIC-C08-AUTH-CODE",
                                       "expiration": "2027-01-01T00:00:00Z", "revoked": False, "status": "FUTURE_STATUS"}]}
        if path == "/aisec/v1/mgmt/deploymentprofiles":
            return 200, {"deployment_profiles": [{"dp_name": f"dp-{n}", "auth_code": "SYNTHETIC-C08-DP-CODE", "status": "active"}
                                                 for n in range(3)], "status": "success"}
        if path == "/dlp/v2/api/data-patterns":
            page = int(query.get("page", 0))
            return 200, {"content": [{"id": f"pattern-{page}", "name": "Card numbers"}], "last": page >= 1,
                         "totalElements": 2, "number": page}
        return 404, {"error": "not found"}

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

    def __init__(self, port, directory, client_id, secret, tsg):
        super().__init__(("127.0.0.1", port), Handler)
        self.directory, self.client_id, self.secret, self.tsg = directory, client_id, secret, tsg
        self.log = os.path.join(directory, "requests.jsonl")
        self.lock = threading.Lock()
        self.issued = set()
        self.counter = 0

    def mode(self):
        try:
            with open(os.path.join(self.directory, "mode.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--certs", required=True)
    parser.add_argument("--dir", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--client-id", required=True)
    parser.add_argument("--secret", required=True)
    parser.add_argument("--tsg", required=True)
    args = parser.parse_args()
    os.makedirs(args.dir, exist_ok=True)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(os.path.join(args.certs, "leaf.pem"), os.path.join(args.certs, "leaf.key"))
    server = Server(args.port, args.dir, args.client_id, args.secret, args.tsg)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    print("ready", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
