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
"topics": "ok"|"deny"|"empty", "profileCount": n, "delayMs": {"<path prefix>": ms},
"topicDescription": text (changes the stored topic, as another writer would)})
and requests.jsonl.

Sessions reads (C11) serve a generated inventory of mode "sessionCount"
sessions (default 60; every mode "detectorEvery"-th, default 7, reports
prompt_injection), their sessions, transactions and scan content. Content
reads are logged by scan ID only, never their text.

Home/Inbox reads (C12): the sessions chart (mode "chartStatus", default
200; two hourly buckets, one violating) and the API key inventory (mode
"keyExpiresInDays" moves key-1's expiration). The public example catalog
snapshot is served at /cdot65/prisma-airs-test-cases/main/catalog.json
(mode "catalog": "ok", "bad" or "missing"); it needs no credential.

Writes (C09) change an in-memory inventory: topics create/update/delete,
API key create/regenerate/delete (a one-time api_key in the reply). The log
records each write's body field names, its authorship and auth-code match,
and only a SHA-256 of any issued one-time key.
"""

import argparse
import base64
import hashlib
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


CONTENT_PROMPT = "SYNTHETIC-C11-PROMPT-CONTENT"
CONTENT_RESPONSE = "SYNTHETIC-C11-RESPONSE-CONTENT"


def session_row(n, every):
    breakdown = [{"detection_type": "prompt_injection", "violation_breakdown": {"total": 1, "high": 1}}] if n % every == 0 else \
        [{"detection_type": "dlp", "violation_breakdown": {"total": 0}}]
    return {"session_id": f"ses-{n:05d}", "application_id": f"app-{n % 3}", "application_name": f"Bot {n % 3}",
            "status": "block" if n % every == 0 else "allow", "detection_type_violation_breakdown": breakdown,
            "start_time": "2026-10-18T10:00:00Z"}


def session_action(session, app, k):
    return {"scan_id": f"scn-{session}-{k}", "scan_sub_req_id": 0, "session_id": session, "application_id": app,
            "status": "block" if k == 0 else "allow", "profile_name": "Production guardrails",
            "detection_type_violation_breakdown": [{"detection_type": "prompt_injection", "violation_breakdown": {"total": 1}}] if k == 0 else [],
            "violation_breakdown": {"total": 1 if k == 0 else 0}, "prompt": CONTENT_PROMPT, "response": CONTENT_RESPONSE}


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

    def do_PUT(self):
        self.handle_request("PUT")

    def do_DELETE(self):
        self.handle_request("DELETE")

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
        self.sent_body = body
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
        if method in ("PUT", "DELETE") or (method == "POST" and url.path != "/oauth2/access_token"):
            try:
                sent = json.loads(body or b"{}")
            except ValueError:
                sent = None
            if isinstance(sent, dict):
                record["fields"] = sorted(sent)
                record["author"] = sent.get("updated_by") or sent.get("created_by")
                record["authCodeMatches"] = sent.get("auth_code") == srv.auth_code if "auth_code" in sent else None
                record["description"] = sent.get("description")
        status, payload = self.write(method, url.path, query, record, mode) if method != "GET" and url.path != "/oauth2/access_token" else self.reply(url.path, query, record, mode)
        record["status"] = status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        self.respond(status, json.dumps(payload).encode() if payload is not None else b"")

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
                # Kept across a restart (offline and back), like the service.
                with open(os.path.join(srv.directory, "issued.json"), "w") as f:
                    json.dump(sorted(srv.issued), f)
            return 200, {"access_token": token, "token_type": "Bearer", "expires_in": 3600}
        if path == "/cdot65/prisma-airs-test-cases/main/catalog.json":
            record["catalogAuthorization"] = record["auth"]
            catalog = mode.get("catalog", "ok")
            if catalog == "missing":
                return 404, {"error": "not found"}
            if catalog == "bad":
                return 200, {"version": 1, "kind": "prisma-airs-test-case-examples", "examples": [], "api_key": "x"}
            return 200, {"version": 1, "kind": "prisma-airs-test-case-examples", "examples": [
                {"product": "skill-scanning", "name": "Public snapshot example", "prompt": "Review the skill's declared permissions.",
                 "expected": "A goal for the reviewer; never a verdict."}]}
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
            with srv.lock:
                if mode.get("topicDescription") and srv.topics:
                    srv.topics[0]["description"] = mode["topicDescription"]
                return 200, {"custom_topics": [dict(t) for t in srv.topics[offset:offset + limit]]}
        if path == f"/aisec/v1/mgmt/customerapp/tsg/{tsg}":
            with srv.lock:
                return 200, {"customer_apps": [dict(a) for a in srv.apps[offset:offset + limit]]}
        if path == f"/aisec/v1/mgmt/apikeys/tsg/{tsg}":
            with srv.lock:
                keys = [dict(k) for k in srv.keys[offset:offset + limit]]
            if mode.get("keyExpiresInDays") is not None and keys:
                when = time.time() + float(mode["keyExpiresInDays"]) * 86400
                keys[0]["expiration"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(when))
            return 200, {"api_keys": keys}
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessionschart":
            status = int(mode.get("chartStatus", 200))
            if status != 200:
                return status, {"message": "SERVER-TEXT-NEVER-SHOWN"}
            return 200, {"bucket_size_seconds": 3600, "buckets": [
                {"time": "2026-10-19T00:00:00Z", "total": 12, "violating": 3,
                 "detection_type_violation_breakdown": [{"detection_type": "pi", "violation_breakdown": {"total": 3}}]},
                {"time": "2026-10-19T01:00:00Z", "total": 8, "violating": 0, "detection_type_violation_breakdown": []}]}
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessionsoverview":
            count = int(mode.get("sessionCount", 60))
            every = int(mode.get("detectorEvery", 7))
            end = min(offset + limit, count)
            return 200, {"items": [session_row(n, every) for n in range(offset, end)], "pagination": {"total_items": count}}
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/session":
            session, app = query.get("session_id", ""), query.get("app_id", "")
            return 200, {"session_id": session, "application_id": app, "application_name": query.get("app_name", ""),
                         "session_actions": [session_action(session, app, k) for k in range(3)]}
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessiontransaction":
            return 200, {"scan_id": query.get("scan_id"), "scan_sub_req_id": 0, "verdict": "malicious",
                         "profile_name": "Production guardrails", "prompt": CONTENT_PROMPT, "response": CONTENT_RESPONSE}
        if path == "/aisec/v1/mgmt/reports/scancontent":
            record["contentScan"] = query.get("scan_id")
            if query.get("scan_id") == "scn-missing":
                return 404, {"error": "not found"}
            return 200, {"scan_id": query.get("scan_id"), "report_id": "R-C11", "category": "malicious",
                         "scan_contents": {"prompt": CONTENT_PROMPT, "response": CONTENT_RESPONSE}}
        if path == "/aisec/v1/mgmt/deploymentprofiles":
            return 200, {"deployment_profiles": [{"dp_name": f"dp-{n}", "auth_code": srv.auth_code if n == 0 else f"SYNTHETIC-C08-DP-CODE-{n}",
                                                  "status": "active"} for n in range(3)], "status": "success"}
        if path.startswith("/dlp/v2/api/"):
            rest = path[len("/dlp/v2/api/"):]
            collection, _, item_id = rest.partition("/")
            if mode.get("patternName") and collection == "data-patterns":
                with srv.lock:
                    srv.dlp["data-patterns"][0]["name"] = mode["patternName"]
            with srv.lock:
                store = srv.dlp.get(collection)
                if store is None:
                    return 404, {"error": "not found"}
                if item_id:
                    found = next((dict(i) for i in store if i["id"] == item_id), None)
                    return (200, found) if found else (404, {"error": "not found"})
                page = int(query.get("page", 0))
                if collection == "data-patterns":
                    # One item per page, whatever the size asked (C08's
                    # Spring paging check: "Items 1-1 of 2").
                    content = [dict(store[page])] if page < len(store) else []
                    return 200, {"content": content, "last": page >= len(store) - 1,
                                 "totalElements": len(store), "number": page}
                return 200, {"content": [dict(i) for i in store], "last": True, "totalElements": len(store)}
        return 404, {"error": "not found"}

    def dlp_write(self, method, path, record, mode):
        """DLP writes (C10): JSON bodies, or a dictionary's multipart upload."""
        srv = self.server
        collection, _, item_id = path[len("/dlp/v2/api/"):].partition("/")
        if mode.get("dlpDeny") == collection and method != "DELETE":
            return 403, {"message": "SERVER-TEXT-NEVER-SHOWN"}
        if collection == "dictionaries" and method in ("POST", "PUT"):
            parts = multipart(self.headers.get("content-type") or "", self.sent_body)
            record["parts"] = [{"name": p["name"], "filename": p["filename"], "contentType": p["contentType"],
                                "sha256": hashlib.sha256(p["body"]).hexdigest(), "bytes": len(p["body"])} for p in parts]
            record["contentType"] = (self.headers.get("content-type") or "").split(";")[0]
            record["boundaryPrefix"] = "boundary=scm-sdk-" in (self.headers.get("content-type") or "")
            meta = next((p for p in parts if p["name"] == "json"), None)
            try:
                body = json.loads(meta["body"]) if meta else {}
            except ValueError:
                body = {}
        else:
            try:
                body = json.loads(self.sent_body or b"{}")
            except ValueError:
                body = {}
            record["dlpBody"] = body if collection != "dictionaries" else None
        with srv.lock:
            store = srv.dlp.get(collection)
            if store is None:
                return 404, {"error": "not found"}
            if method == "POST" and not item_id:
                srv.serial += 1
                item = dict(body, id=f"{collection}-{srv.serial}")
                store.append(item)
                return 200, item
            index = next((n for n, i in enumerate(store) if i["id"] == item_id), None)
            if index is None:
                return 404, {"error": "not found"}
            if method == "PUT":
                store[index] = dict(body, id=item_id)
                return (204, None) if collection == "dictionaries" else (200, store[index])
            if method == "DELETE":
                store.pop(index)
                return 204, None
        return 405, {"error": "method"}

    def write(self, method, path, query, record, mode):
        srv = self.server
        if record["auth"] != "bearer-issued":
            return 401, {"error": "unauthorized"}
        if path.startswith("/dlp/v2/api/"):
            return self.dlp_write(method, path, record, mode)
        try:
            body = json.loads(self.sent_body or b"{}")
        except ValueError:
            body = {}
        with srv.lock:
            if method == "POST" and path == "/aisec/v1/mgmt/topic":
                srv.serial += 1
                topic = dict(body, topic_id=f"33333333-3333-4333-8333-{srv.serial:012d}", revision=1)
                topic.setdefault("description", "")
                srv.topics.append(topic)
                return 200, topic
            if method == "PUT" and path.startswith("/aisec/v1/mgmt/topic/uuid/"):
                tid = path.rsplit("/", 1)[1]
                for i, t in enumerate(srv.topics):
                    if t["topic_id"] == tid:
                        updated = dict(body, topic_id=tid, revision=t.get("revision", 0) + 1)
                        updated.setdefault("description", "")
                        srv.topics[i] = updated
                        return 200, updated
                return 404, {"error": "not found"}
            if method == "DELETE" and path.startswith("/aisec/v1/mgmt/topic/"):
                tid = path.rsplit("/", 1)[1]
                srv.topics = [t for t in srv.topics if t["topic_id"] != tid]
                return 200, {"message": "deleted"}
            if method == "POST" and path == "/aisec/v1/mgmt/apikey":
                srv.serial += 1
                secret = f"SYNTHETIC-C09-ONE-TIME-{srv.serial:04d}-{base64.b32encode(hashlib.sha256(str(srv.serial).encode()).digest()[:10]).decode()}"
                key = {"api_key_id": f"key-{srv.serial + 1}", "api_key_name": body.get("api_key_name"),
                       "cust_app": body.get("cust_app"), "api_key_last8": secret[-8:],
                       "expiration": "2027-06-01T00:00:00Z", "revoked": False, "auth_code": body.get("auth_code", "")}
                srv.keys.append(key)
                if not any(a["app_name"] == body.get("cust_app") for a in srv.apps):
                    srv.apps.append({"customer_appId": f"app-{srv.serial + 1}", "tsg_id": srv.tsg, "app_name": body.get("cust_app"),
                                     "cloud_provider": body.get("cust_cloud_provider"), "environment": body.get("cust_env")})
                record["issuedSha256"] = hashlib.sha256(secret.encode()).hexdigest()
                return 200, dict(key, api_key=secret)
            if method == "POST" and path.startswith("/aisec/v1/mgmt/apikey/regenerate/"):
                kid = path.rsplit("/", 1)[1]
                for key in srv.keys:
                    if key["api_key_id"] == kid:
                        srv.serial += 1
                        secret = f"SYNTHETIC-C09-ROTATED-{srv.serial:04d}-{base64.b32encode(hashlib.sha256(str(srv.serial).encode()).digest()[:10]).decode()}"
                        key["api_key_last8"] = secret[-8:]
                        record["issuedSha256"] = hashlib.sha256(secret.encode()).hexdigest()
                        return 200, dict(key, api_key=secret)
                return 404, {"error": "not found"}
            if method == "DELETE" and path.startswith("/aisec/v1/mgmt/apikey/delete/"):
                name = path.rsplit("/", 1)[1].replace("%20", " ")
                srv.keys = [k for k in srv.keys if k["api_key_name"] != name]
                return 200, {"message": "deleted"}
        return 404, {"error": "not found"}

    def respond(self, status, payload):
        if status == 204:
            self.send_response(204)
            self.send_header("content-length", "0")
            self.send_header("connection", "close")
            self.end_headers()
            self.close_connection = True
            return
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)
        self.close_connection = True


def multipart(content_type, body):
    """An independent multipart/form-data reader for the request log."""
    boundary = content_type.split("boundary=")[-1].strip('"') if "boundary=" in content_type else None
    if not boundary:
        return []
    parts = []
    for chunk in body.split(("--" + boundary).encode())[1:-1]:
        chunk = chunk.removeprefix(b"\r\n").removesuffix(b"\r\n")
        head, _, content = chunk.partition(b"\r\n\r\n")
        part = {"name": None, "filename": None, "contentType": None, "body": content}
        for line in head.decode("utf-8", "replace").split("\r\n"):
            lower = line.lower()
            if lower.startswith("content-disposition:"):
                for field in [f.strip() for f in line.split(";")]:
                    if field.startswith("name="):
                        part["name"] = field[5:].strip('"')
                    if field.startswith("filename="):
                        part["filename"] = field[9:].strip('"')
            elif lower.startswith("content-type:"):
                part["contentType"] = line.split(":", 1)[1].strip()
        parts.append(part)
    return parts


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port, directory, client_id, secret, tsg):
        super().__init__(("127.0.0.1", port), Handler)
        self.directory, self.client_id, self.secret, self.tsg = directory, client_id, secret, tsg
        self.log = os.path.join(directory, "requests.jsonl")
        self.lock = threading.Lock()
        try:
            with open(os.path.join(directory, "issued.json")) as f:
                self.issued = set(json.load(f))
        except (OSError, ValueError):
            self.issued = set()
        self.counter = len(self.issued) + 1000 * int(time.time() % 1000)
        self.serial = 0
        self.dlp = {
            "data-patterns": [{"id": "pattern-1", "name": "Card numbers", "type": "custom",
                               "detection_config": {"technique": "regex"}},
                              {"id": "pattern-2", "name": "Badge numbers", "type": "custom",
                               "detection_config": {"technique": "regex"}}],
            "data-profiles": [{"id": "dp-1", "name": "PCI", "detection_rules": []}],
            "data-filtering-profiles": [{"id": "filter-1", "name": "Default", "file_based": True, "non_file_based": False}],
            "dictionaries": [{"id": "dict-1", "name": "Codes", "original_file_name": "codes.txt",
                              "category": "Confidential", "region_name": "United States"}],
        }
        self.auth_code = "SYNTHETIC-C08-DP-CODE"
        self.topics = [{"topic_id": "11111111-1111-4111-8111-111111111111", "topic_name": "Finance",
                        "revision": 1, "description": "Money talk"}]
        self.apps = [{"customer_appId": "app-1", "tsg_id": tsg, "app_name": "Support bot",
                      "cloud_provider": "aws", "environment": "production"}]
        self.keys = [{"api_key_id": "key-1", "api_key_name": "Runtime", "api_key_last8": "abcd1234",
                      "api_key": "SYNTHETIC-C08-KEY-VALUE", "auth_code": "SYNTHETIC-C08-AUTH-CODE", "cust_app": "Support bot",
                      "expiration": "2027-01-01T00:00:00Z", "revoked": False, "status": "FUTURE_STATUS"}]

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
