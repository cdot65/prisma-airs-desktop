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

Gateway writes (C17) change an overlay the Gateway reads consult: provider
renames and deletes, created service keys (a one-time key in the reply,
logged only as a SHA-256), created IAM scopes and their bindings, and
provisioned workspaces. Modes gwLose ("METHOD path": apply, then close
without a reply) and gwHang ("METHOD path": never reply) make a step's
outcome unknown.

Gateway observation and inference (C18): the data plane answers usage
charts (requests, cost, tokens, latency: mode "gwChartPoints" hourly
points, default 24), the model groups (mode "gwModelRows", default 3),
request logs (25 a page; a trace or status filter is honoured) and the
usage- and rate-limit policies of a workspace (mode "gwTelemetry":
"ok" or "fail" makes the cost chart answer 500). The runtime endpoint
POST /rt/v1/chat/completions streams a chat completion as server-sent
events according to mode "chat": "text" (two deltas), "bulk" (64 KiB in
2,048 chunks of 32 bytes), "paced" (one 32-byte chunk every 10 ms, 2,048
times), "hang" (one chunk, then nothing until the client goes away) or
"status:<code>". POST /rt/v1/responses streams a Responses run (created, two text deltas,
completed; mode "responses": "incomplete" ends it with response.incomplete
for max_output_tokens after one delta, "failed" with response.failed; the
same "status:<code>" refusals).
The log records the run's headers by name, the inference key only as a
SHA-256, the trace and route, the body's field names and model and an
attached input_file's ID, never the prompt. A Responses request from the
native Harness engine (C19: it carries instructions, tools and the
x-codex-turn-metadata header) is answered as the engine expects (created,
output_item.added, text deltas "Answer <n>: Hello from the synthetic
Gateway.", output_item.done, completed with usage;
mode "harness": "short", the default, "paced", one 32-byte delta every
50 ms for about 20 s, until the client goes away, or "kib", a 1 KiB answer
at full speed). C21: mode "harnessTool" ({namespace, name, arguments})
makes it ask for that tool call instead, until the request carries the
call's output (logged as "toolOutput": size, SHA-256, whether it carries
the management secret, and which "toolMarkers" it names); its log row says
"harness": true and records the engine's sandbox mode from its turn
metadata. /rt/v1/files lists, uploads
(multipart) and deletes the workspace's files (mode "files": "ok",
"status:<code>" or "lost"); the log records an upload's file name, purpose,
size and SHA-256, never its content.

Workspace MCP (C22): GET /ai_gw/v2/mcp-servers lists Team A's two servers
(Tools, Docs; mode "mcpCatalog": "down" answers 500, "mcpUpdated" changes
their update time and so their revision, "mcpTag" tags their names). Each
server's Streamable HTTP endpoint /mcp/<slug>/mcp requires a bearer the
plane issued for its URL, else answers 401 with a resource_metadata
challenge; initialize, tools/list (one tool, lookup) and tools/call. The
OAuth surface: protected-resource metadata, authorization-server metadata
at /.well-known/oauth-authorization-server/mcp-oauth, /mcp-oauth/register,
/mcp-oauth/authorize (302 to the loopback with a code bound to the PKCE
challenge, client, redirect and resource), /mcp-oauth/token (code and
refresh with rotation; mode "mcpLifetime", "mcpRefresh": "refuse") and
/mcp-oauth/revoke. Mode "harnessMcp" ({server, tool, arguments}) makes the
Gateway ask for that server's tool by the engine's own name for it. Tokens
are never logged; an MCP request's bearer is logged as issued, placeholder,
other or none.

Company sign-in (C20): a synthetic OpenID provider under /idp: discovery,
JWKS (one P-256 key made at start with the system openssl), /idp/authorize
(the browser step: it answers 302 to the app's loopback redirect with a
code bound to the PKCE challenge and nonce), /idp/token (authorization code
with S256, and refresh with rotation) and /idp/revoke. Tokens are ES256
JWTs; mode "idpSubject" names the account (default "native-c20-alice"),
"idpLifetime" the access token's seconds (default 3600) and "idpRefresh":
"refuse" refuses renewals. The log records grant types, subjects and
SHA-256s of tokens only. A Harness request records whether its key is an
access token this provider issued ("keyIssuedByIdp") and whether its body
carries the marker "C20-CANARY" ("carriesCanary"), never the prompt.
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
import subprocess
from urllib.parse import parse_qs, quote, urlencode, urlsplit

TOKEN_PREFIX = "c08-mgmt-access-"
# C16 synthetic Gateway workspaces and the tenant-wide integration.
GW_A = "16f7e90d-382a-4e78-b577-1b01eb5f8297"
GW_B = "16f7e90d-382a-4e78-b577-1b01eb5f8298"
GW_INTEGRATION = "f6692544-3265-49be-9711-bbdcebc079eb"
# A second tenant-wide integration, listed only with mode gwIntegrations=2.
GW_INTEGRATION_B = "0b6f3a10-5c2d-4e8a-9f1b-2c3d4e5f6a7b"


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
            "xTsgId": self.headers.get("x-tsg-id"),
            "scanToken": self.headers.get("x-pan-token") is not None,
        }
        if url.path.startswith("/idp/"):
            self.idp(method, url.path[4:], query, body, record, mode)
            return
        if (url.path.startswith(("/mcp/", "/mcp-oauth/", "/.well-known/oauth-"))
                or (method == "GET" and url.path == "/ai_gw/v2/mcp-servers")):
            self.mcp(method, url.path, query, body, record, mode)
            return
        if method == "POST" and url.path in ("/rt/v1/chat/completions", "/rt/v1/responses"):
            self.inference(body, record, mode, responses=url.path.endswith("/responses"))
            return
        if url.path == "/rt/v1/files" or url.path.startswith("/rt/v1/files/"):
            self.files(method, url.path, body, record, mode)
            return
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
        if status == "logged":
            self.close_connection = True
            return
        record["status"] = status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        if status == "lost":
            # The step took effect; its reply is lost with the connection.
            self.close_connection = True
            return
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
        red_team = self.red_team(path, query, mode)
        if red_team is not None:
            return red_team
        gateway = self.gateway(path, query, mode)
        if gateway is not None:
            return gateway
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

    def red_team(self, path, query, mode):
        """C13: AI Red Teaming, Model Security and AgentGuard reads. Names,
        URIs, findings and prompts are private markers the desktop must never
        show. Modes: rtStatsStatus, rtScans (count), rtScanTotal, attackTotal,
        modelsStatus, skillStatsStatus, skillsStatus (HTTP statuses)."""
        failed = lambda key: int(mode.get(key, 200)) != 200
        if path == "/ai-red-teaming/data-plane/v1/dashboard/scan-statistics":
            if failed("rtStatsStatus"):
                return int(mode["rtStatsStatus"]), {"error": "SERVER-TEXT-NEVER-SHOWN"}
            return 200, {"total_scans": 100, "targets_scanned": 7,
                         "scan_status": [{"name": "COMPLETED", "count": 90}, {"name": "RUNNING", "count": 6},
                                         {"name": "QUEUED", "count": 2}, {"name": "FAILED", "count": 2}],
                         "risk_profile": [{"risk_rating": "HIGH", "total": 2}, {"risk_rating": "LOW", "total": 5}],
                         "targets_scanned_by_type": [{"name": "APPLICATION", "count": 5}, {"name": "MODEL", "count": 2}],
                         "date_range": query.get("date_range")}
        if path == "/ai-red-teaming/data-plane/v1/scan":
            count = min(int(query.get("limit", 100)), int(mode.get("rtScans", 100)))
            statuses = ["COMPLETED", "RUNNING", "FAILED", "QUEUED"]
            rows = [{"uuid": f"{n:08x}-2222-4333-8444-555555555555", "tsg_id": self.server.tsg,
                     "name": f"RT-PRIVATE-NAME-{n}", "target": {"name": "RT-PRIVATE-TARGET"},
                     "target_id": "66666666-7777-4888-8999-aaaaaaaaaaaa", "target_type": "APPLICATION",
                     "job_type": "STATIC", "status": statuses[0] if n < 90 else statuses[n % 4],
                     "created_at": "2026-10-18T10:00:00Z"} for n in range(count)]
            return 200, {"data": rows, "pagination": {"total_items": int(mode.get("rtScanTotal", mode.get("rtScans", 100)))}}
        if path.startswith("/ai-red-teaming/data-plane/v1/report/static/") and path.endswith("/list-attacks"):
            total = int(mode.get("attackTotal", 250))
            skip = int(query.get("skip", 0))
            count = max(0, min(int(query.get("limit", 100)), total - skip))
            rows = [{"uuid": f"{skip + i:08x}-aaaa-4bbb-8ccc-dddddddddddd", "prompt": "RT-PRIVATE-PROMPT",
                     "threat": (skip + i) % 4 == 0, "severity": "HIGH", "attack_type": "NORMAL",
                     "category": "security", "sub_category": "JAILBREAK", "sub_category_display_name": "Jailbreak",
                     "multi_turn": False, "extra_info": {"response": "RT-PRIVATE-REPLY"}} for i in range(count)]
            return 200, {"data": rows, "pagination": {"total_items": total}}
        if path == "/aims/data/v1/scans":
            if failed("modelsStatus"):
                return int(mode["modelsStatus"]), {"error": "SERVER-TEXT-NEVER-SHOWN"}
            rows = [{"uuid": f"{n:08x}-3333-4333-8444-555555555555", "model_uri": "s3://MS-PRIVATE-URI/model",
                     "security_group_name": "MS-PRIVATE-GROUP", "eval_outcome": "BLOCKED" if n % 5 == 0 else "ALLOWED",
                     "created_at": "2026-10-18T10:00:00Z"} for n in range(100)]
            return 200, {"scans": rows, "pagination": {"total_items": 140}}
        if path == "/aiag/data/v1/stats/scans":
            if failed("skillStatsStatus"):
                return int(mode["skillStatsStatus"]), {"error": "SERVER-TEXT-NEVER-SHOWN"}
            return 200, {"unique_skills_scanned": {"count": 4, "percent_change": None},
                         "total_vulnerabilities_found": {"count": 6, "percent_change": 10},
                         "top_vulnerability": {"vulnerability_type": "AG-PRIVATE-FINDING", "finding_count": 3}}
        if path == "/aiag/data/v1/scans":
            if failed("skillsStatus"):
                return int(mode["skillsStatus"]), {"error": "SERVER-TEXT-NEVER-SHOWN"}
            rows = [{"uuid": f"{n:08x}-4444-4333-8444-555555555555", "name": f"AG-PRIVATE-SKILL-{n}",
                     "status": "COMPLETED", "eval_outcome": "ALLOWED", "artifact_type": "skill",
                     "summary": {"vulnerability_count": n % 3, "attack_chain_count": 0},
                     "created_at": "2026-10-18T10:00:00Z"} for n in range(100)]
            return 200, {"scans": rows, "pagination": {"total_items": 100}}
        return None

    def gateway(self, path, query, mode):
        """C16: AI Gateway reads (data plane /ai_gw/v2, admin /ai_gw/admin/v2,
        IAM /iam/v1) in the SDK's contract shapes. Key values, provider keys,
        model configuration and credential-bearing settings carry GW-PRIVATE
        markers the desktop must never show. Modes: gwWorkspaces ("both",
        "onlyA", "onlyB"), gwAdminStatus and gwIamStatus (HTTP statuses);
        delayMs slows any path prefix."""
        if not path.startswith(("/ai_gw/", "/iam/v1/")):
            return None
        stamp = {"created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z"}
        spaces = {GW_A: ("Team A", "team-a"), GW_B: ("Team B", "team-b")}
        listed = {"both": [GW_A, GW_B], "onlyA": [GW_A], "onlyB": [GW_B]}[mode.get("gwWorkspaces", "both")]

        def listing(rows, total=None, more=False):
            return {"object": "list", "total": len(rows) if total is None else total, "success": True,
                    "has_more": more, "data": rows}

        def workspace(ws):
            name, slug = spaces[ws]
            return {"id": ws, "slug": slug, "name": name, "icon": None, "description": None, **stamp, "is_default": 0,
                    "status": "active", "scope_name": f"scope-{slug}", "object": "workspace"}

        def provider(ws, n):
            return {"id": f"7e3a0000-0000-4000-8000-{ws[-4:]}0000000{n}", "name": f"{spaces[ws][1]}-provider-{n}",
                    "slug": f"{spaces[ws][1]}-provider-{n}", "object": "provider", "integration_id": GW_INTEGRATION,
                    "status": "active", "key": "GW-PRIVATE-PROVIDER-KEY",
                    "usage_limits": {"credit_limit": 100, "type": "cost", "current_usage": 7}}

        def route(ws):
            return {"id": f"7e3a0000-0000-4000-8000-{ws[-4:]}00000010", "name": f"{spaces[ws][1]}-route",
                    "slug": f"{spaces[ws][1]}-route", "organisation_id": "org-1", "is_default": 1, "status": "active",
                    "owner_id": "owner-1", "updated_by": "owner-1", **stamp, "workspace_id": ws, "object": "config"}

        def guardrail(ws):
            return {"id": f"7e3a0000-0000-4000-8000-{ws[-4:]}00000020", "name": f"{spaces[ws][1]}-guardrail",
                    "slug": f"{spaces[ws][1]}-guardrail", "organisation_id": "org-1", "status": "active",
                    "owner_id": "owner-1", "updated_by": None, **stamp, "workspace_id": ws, "object": "guardrail"}

        def key(ws, n, scopes):
            return {"id": f"7e3a0000-0000-4000-8000-{ws[-4:]}0000003{n}", "name": f"{spaces[ws][1]}-key-{n}",
                    "object": "api-key", "status": "active", "key": "GW-PRIVATE-KEY-VALUE", "scopes": scopes}

        integration = {"id": GW_INTEGRATION, "name": "OpenAI", "owner_id": "owner-1", "status": "active",
                       "created_at": "2026-10-01T00:00:00Z", "last_updated_at": None, "slug": "openai",
                       "description": None, "workspace_id": None, "ai_provider_id": "openai",
                       "object": "integration", "masked_key": "GW-PRIVATE-MASKED"}
        integration_b = {**integration, "id": GW_INTEGRATION_B, "name": "Anthropic", "slug": "anthropic",
                         "ai_provider_id": "anthropic"}
        integrations = [integration, integration_b] if mode.get("gwIntegrations") == 2 else [integration]
        admin, iam = int(mode.get("gwAdminStatus", 200)), int(mode.get("gwIamStatus", 200))
        gw = self.server.gw
        with self.server.lock:
            created_ws = [dict(w) for w in gw["workspaces"] if w["id"] not in gw["archived"]]
            created = {k: [dict(i) for i in v if i["id"] not in gw["deleted"]] for k, v in gw["created"].items()}
            renamed, deleted, scopes = dict(gw["renamed"]), set(gw["deleted"]), {k: dict(v) for k, v in gw["scopes"].items()}
        if path.startswith("/ai_gw/admin/v2/"):
            if admin != 200:
                return admin, {"error": "SERVER-TEXT-NEVER-SHOWN"}
            rest = path[len("/ai_gw/admin/v2"):]
            if rest == "/workspaces":
                # The admin inventory (C17 reconciliation reads it).
                return 200, listing([workspace(w) for w in listed] + created_ws)
            if rest == "/integrations":
                return 200, listing(integrations)
            if rest == f"/integrations/{GW_INTEGRATION_B}" and len(integrations) == 2:
                return 200, {**integration_b, "configurations": json.dumps(
                    {"api_key": "GW-PRIVATE-INTEGRATION-SECRET", "region": "us-west-2"})}
            if rest == f"/integrations/{GW_INTEGRATION_B}/models" and len(integrations) == 2:
                return 200, {"object": "list", "allow_all_models": False, "total": 1, "models": [
                    {"slug": "claude-sonnet", "enabled": True, "name": "Claude Sonnet"}]}
            if rest == f"/integrations/{GW_INTEGRATION}":
                return 200, {**integration, "configurations": json.dumps(
                    {"api_key": "GW-PRIVATE-INTEGRATION-SECRET", "region": "us-east-1"})}
            if rest == f"/integrations/{GW_INTEGRATION}/models":
                return 200, {"object": "list", "allow_all_models": False, "total": 2, "models": [
                    {"slug": "gpt-4o", "enabled": True, "name": "GPT-4o"}, {"slug": "o3", "enabled": False}]}
            return 404, {"error": "not found"}
        if path.startswith("/iam/v1/scopes/"):
            if iam != 200:
                return iam, {"error": "SERVER-TEXT-NEVER-SHOWN"}
            name = path[len("/iam/v1/scopes/"):]
            if name in scopes:
                return 200, {"name": name, "description": "", "tsg_id": self.server.tsg, "id": f"id-{name}",
                             "resources": scopes[name]["resources"]}
            if name not in ("scope-team-a", "scope-team-b"):
                return 404, {"error": "not found"}
            # Team A's scope binds it; Team B's binds someone else.
            bound = "team-a" if name == "scope-team-a" else "someone-else"
            return 200, {"name": name, "description": "", "tsg_id": self.server.tsg, "id": f"id-{name}",
                         "resources": [{"resource_type": "workspace", "resource_id": bound}]}
        rest = path[len("/ai_gw/v2"):]
        ws = query.get("workspace_id")
        observed = self.observation(rest, query, mode, spaces)
        if observed is not None:
            return observed
        if rest == "/workspaces":
            return 200, listing([workspace(w) for w in listed] + created_ws)
        if rest.startswith("/workspaces/"):
            wid = rest[len("/workspaces/"):]
            base = workspace(wid) if wid in spaces else next((w for w in created_ws if w["id"] == wid), None)
            if base is None:
                return 404, {"error": "not found"}
            return 200, {**base, "defaults": {"config_id": route(GW_A)["id"], "allow_config_override": True},
                         "usage_limits": [{"credit_limit": 500, "type": "cost", "current_usage": 12}],
                         "rate_limits": [{"type": "requests", "unit": "rpm", "value": 60}]}
        if ws is not None and ws not in spaces:
            return 200, listing([])

        def shown(p):
            return {**p, "name": renamed.get(p["id"], p["name"])}
        if rest == "/providers":
            # Team B's providers are a partial page: 1 of 5, more follow.
            if ws == GW_A:
                return 200, listing([shown(p) for p in (provider(ws, 1), provider(ws, 2)) if p["id"] not in deleted])
            return 200, listing([provider(ws, 1)], 5, True)
        if rest == "/configs":
            return 200, listing([route(ws)])
        if rest == "/guardrails":
            return 200, listing([guardrail(ws)])
        if rest == "/api-keys/service":
            return 200, listing([key(ws, 1, ["completions.write"]), key(ws, 2, ["models.read"])]
                                + [k for k in created["api-keys/service"] if k["workspace_id"] == ws])
        for k in created["api-keys/service"]:
            if rest == f"/api-keys/service/{k['id']}":
                return 200, k
        if rest == "/api-keys/user":
            return 200, listing([key(ws, 3, ["completions.write"])])
        for w in spaces:
            r = route(w)
            if rest == f"/configs/{r['id']}":
                return 200, {**r, "config": json.dumps({"strategy": {"mode": "fallback"}, "targets": [
                    {"provider": f"@{spaces[w][1]}-provider-1", "api_key": "GW-PRIVATE-ROUTE-KEY",
                     "forward_headers": ["x-trace"]},
                    {"provider": "@backup", "weight": 1}]}), "format": "json", "type": "ROUTING", "version_id": "v-1"}
            g = guardrail(w)
            if rest == f"/guardrails/{g['id']}":
                return 200, {**g, "checks": [{"id": "pii", "parameters": {"apiKey": "GW-PRIVATE-GUARDRAIL-KEY"},
                                              "is_enabled": True}],
                             "actions": {"deny": True, "async": False, "sequential": False}, "version_id": "v-1"}
            for n in (1, 2):
                p = provider(w, n)
                if rest == f"/providers/{p['id']}":
                    if p["id"] in deleted:
                        return 404, {"error": "not found"}
                    return 200, {"id": p["id"], "ai_provider_name": "openai",
                                 "model_config": {"api_key": "GW-PRIVATE-MODEL-CONFIG"}, "key": "GW-PRIVATE-PROVIDER-KEY",
                                 "masked_api_key": "GW-PRIVATE-MASKED", "slug": p["slug"],
                                 "name": renamed.get(p["id"], p["name"]),
                                 "usage_limits": None, "status": "active", "note": None,
                                 "created_at": "2026-10-01T00:00:00Z", "expires_at": None, "last_reset_at": None,
                                 "rate_limits": [], "integration_id": GW_INTEGRATION, "object": "provider"}
        return 404, {"error": "not found"}

    def observation(self, rest, query, mode, spaces):
        """C18: telemetry, logs and limit policies of one workspace."""
        slug = query.get("workspaceSlug")
        points = int(mode.get("gwChartPoints") or 24)

        def records(scale):
            return [{"x": f"2026-10-{1 + i // 24:02d}T{i % 24:02d}:00:00Z", "y": scale} for i in range(points)]
        quota = {"isQuotaExceeded": False}
        if rest == "/logs/charts/requests":
            return 200, {"success": True, "data": {"records": records(5), "total": 5 * points,
                                                   "total_llm_count": 5 * points, "total_llm_error_count": points, **quota}}
        if rest == "/logs/charts/cost":
            if (mode.get("gwTelemetry") or "ok") == "fail":
                return 500, {"error": "synthetic failure"}
            return 200, {"success": True, "data": {"records": records(30), "total": 30 * points, "avg": 30, **quota}}
        if rest == "/logs/charts/tokens":
            rows = [{**r, "total_request_units": 40, "total_response_units": 80, "avg": 24} for r in records(120)]
            return 200, {"success": True, "data": {"records": rows, "total": 120 * points, "avg": 24,
                                                   "total_request_units": 40 * points, "total_response_units": 80 * points, **quota}}
        if rest == "/logs/charts/latency":
            rows = [{**r, "p50": 300, "p90": 500, "p99": 900} for r in records(350)]
            return 200, {"success": True, "data": {"records": rows, "total": 350, "p50": 300, "p90": 500, "p99": 900, **quota}}
        if rest == "/logs/groups/model":
            n = int(mode.get("gwModelRows") or 3)
            rows = [{"object": "group", "model": f"model-{i:03d}", "ai_org": "openai", "requests": 1000 - i,
                     "cost": 10 * (1000 - i), "total_tokens": 50 * (1000 - i), "success_rate": 0.98} for i in range(n)]
            return 200, {"object": "list", "is_quota_exceeded": False, "total": n, "data": rows}
        if rest == "/logs":
            trace = query.get("traceId")
            status = query.get("statusCode")
            rows = []
            for i in range(3):
                code = 446 if i == 1 else 200
                rows.append({"id": f"log-{i}", "workspace_slug": slug, "ai_model": "gpt-4o", "_user": "synthetic-user",
                             "total_units": 120, "cost": 0.4, "trace_id": trace or f"airs-fixture-{i}", "is_proxy_call": 0,
                             "created_at": f"2026-10-20T0{i}:00:00Z", "is_success": 1 if code < 400 else 0,
                             "cache_status": "MISS", "retry_success_count": 0, "mode": "single",
                             "last_used_option_index": 0, "response_status_code": code,
                             "request_url": "https://runtime.invalid/v1/chat/completions", "request_method": "post",
                             "ai_org": "openai", "api_key_id": "key-1", "license_id": "l-1",
                             "log_store_file_path_format": "v1", "metadataKey": [], "metadataValue": [],
                             "prompt_slug": "", "feedback": []})
            if trace:
                rows = rows[:1]
            if status:
                rows = [r for r in rows if str(r["response_status_code"]) == status]
            return 200, {"success": True, "data": {"records": rows, "total": len(rows), "capturedTotal": len(rows), **quota}}
        if rest in ("/policies/usage-limits", "/policies/rate-limits"):
            ws = query.get("workspace_id")
            if ws not in spaces:
                return 200, {"object": "list", "total": 0, "data": []}
            base = {"status": "active", "workspace_id": ws, "organisation_id": self.server.tsg,
                    "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-01T00:00:00Z",
                    "group_by": [{"key": "api_key"}]}
            if rest == "/policies/usage-limits":
                row = {**base, "id": "ul-1", "name": "Monthly budget", "type": "cost", "credit_limit": 5000,
                       "alert_threshold": 80, "periodic_reset": "monthly", "next_usage_reset_at": "2026-11-01T00:00:00Z"}
            else:
                row = {**base, "id": "rl-1", "name": "Requests per minute", "type": "requests", "unit": "rpm", "value": 60}
            return 200, {"object": "list", "total": 1, "data": [row]}
        return None

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
        if path.startswith(("/ai_gw/", "/iam/v1/")):
            return self.gateway_write(method, path, record, mode)
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

    def gateway_write(self, method, path, record, mode):
        """C17: Gateway and IAM writes against the overlay the reads consult.
        A lost step is applied, then its connection closed without a reply;
        a hung step never replies."""
        srv = self.server
        step = f"{method} {path}"
        if mode.get("gwHang") == step:
            # Logged now: the step never replies.
            record["hung"] = True
            record["status"] = "hung"
            with srv.lock, open(srv.log, "a") as f:
                f.write(json.dumps(record) + "\n")
            time.sleep(120)
            return "logged", None
        try:
            body = json.loads(self.sent_body or b"{}")
        except ValueError:
            body = {}
        record["gatewayBody"] = {k: v for k, v in body.items() if k not in ("key", "secret_mappings")} if isinstance(body, dict) else None
        gw = srv.gw
        status, payload = 404, {"error": "not found"}
        with srv.lock:
            if method == "PUT" and path.startswith("/ai_gw/v2/providers/"):
                pid = path.rsplit("/", 1)[1]
                if "name" in body:
                    gw["renamed"][pid] = body["name"]
                status, payload = 200, {"success": True, "id": pid}
            elif method == "DELETE" and path.startswith("/ai_gw/v2/providers/"):
                gw["deleted"].add(path.rsplit("/", 1)[1])
                status, payload = 204, None
            elif method == "POST" and path == "/ai_gw/v2/api-keys/service":
                srv.serial += 1
                kid = f"7e3a0000-0000-4000-8000-{srv.serial:012d}"
                secret = f"GW-ONE-TIME-{srv.serial:04d}-{base64.b32encode(hashlib.sha256(str(srv.serial).encode()).digest()[:10]).decode()}"
                gw["created"]["api-keys/service"].append({"id": kid, "name": body.get("name"), "object": "api-key",
                                                          "status": "active", "key": "GW-PRIVATE-KEY-VALUE",
                                                          "scopes": body.get("scopes", []), "workspace_id": body.get("workspace_id")})
                record["issuedSha256"] = hashlib.sha256(secret.encode()).hexdigest()
                status, payload = 200, {"id": kid, "success": True, "key": secret, "object": "api-key"}
            elif method == "POST" and path == "/iam/v1/scopes":
                name = body.get("name", "")
                gw["scopes"][name] = {"resources": []}
                status, payload = 200, {"name": name, "description": body.get("description", ""), "tsg_id": srv.tsg,
                                        "id": f"id-{name}", "resources": []}
            elif method == "PUT" and path.startswith("/iam/v1/scopes/"):
                name = path.rsplit("/", 1)[1]
                if name in gw["scopes"]:
                    gw["scopes"][name]["resources"] = body.get("resources", [])
                    status, payload = 200, {"name": name, "description": body.get("description", ""), "tsg_id": srv.tsg,
                                            "id": f"id-{name}", "resources": gw["scopes"][name]["resources"]}
            elif method == "DELETE" and path.startswith("/iam/v1/scopes/"):
                gw["scopes"].pop(path.rsplit("/", 1)[1], None)
                status, payload = 204, None
            elif method == "POST" and path == "/ai_gw/admin/v2/workspaces":
                srv.serial += 1
                wid = f"16f7e90d-382a-4e78-b577-{srv.serial:012d}"
                name = body.get("name", "Workspace")
                slug = name.lower().replace(" ", "-")
                gw["workspaces"].append({"id": wid, "slug": slug, "name": name, "icon": None, "description": None,
                                         "created_at": "2026-10-20T00:00:00Z", "last_updated_at": "2026-10-20T00:00:00Z",
                                         "is_default": 0, "status": "active", "scope_name": body.get("scope_name"),
                                         "object": "workspace"})
                status, payload = 200, {"id": wid, "name": name, "slug": slug, "description": None,
                                        "created_at": "2026-10-20T00:00:00Z", "last_updated_at": "2026-10-20T00:00:00Z",
                                        "scope_name": body.get("scope_name"), "object": "workspace"}
        if mode.get("gwLose") == step:
            record["lost"] = True
            return "lost", None
        return status, payload

    def inference(self, body, record, mode, responses=False):
        """C18: one streamed chat completion or Responses run (never the
        prompt in the log)."""
        srv = self.server
        key = self.headers.get("x-portkey-api-key") or ""
        record["inferenceKeySha256"] = hashlib.sha256(key.encode()).hexdigest() if key else None
        record["trace"] = self.headers.get("x-portkey-trace-id")
        record["route"] = self.headers.get("x-portkey-config")
        record["authorizationHeader"] = self.headers.get("authorization") is not None
        try:
            sent = json.loads(body or b"{}")
        except ValueError:
            sent = {}
        record["fields"] = sorted(sent) if isinstance(sent, dict) else None
        record["model"] = sent.get("model") if isinstance(sent, dict) else None
        record["stream"] = sent.get("stream") if isinstance(sent, dict) else None
        record["maxTokens"] = sent.get("max_tokens") if isinstance(sent, dict) else None
        record["messages"] = len(sent.get("messages") or []) if isinstance(sent, dict) else None
        record["store"] = sent.get("store") if isinstance(sent, dict) else None
        record["maxOutputTokens"] = sent.get("max_output_tokens") if isinstance(sent, dict) else None
        attached = [part.get("file_id") for item in (sent.get("input") if isinstance(sent, dict) and isinstance(sent.get("input"), list) else [])
                    for part in (item.get("content") or []) if isinstance(part, dict) and part.get("type") == "input_file"]
        record["inputFiles"] = attached
        with srv.lock:
            record["keyIssuedByIdp"] = key in srv.idp_access
        record["carriesCanary"] = b"C20-CANARY" in body
        # C21: what the engine sent back for a tool call (its last input
        # item): never the text, only its size, digest, whether it carries
        # the management secret and which of mode "toolMarkers" it names.
        items = sent.get("input") if isinstance(sent, dict) and isinstance(sent.get("input"), list) else []
        last = items[-1] if items and isinstance(items[-1], dict) else {}
        if last.get("type") == "function_call_output":
            output = last.get("output")
            text = output if isinstance(output, str) else json.dumps(output)
            record["toolOutput"] = {"bytes": len(text.encode()), "sha256": hashlib.sha256(text.encode()).hexdigest(),
                                    "carriesSecret": srv.secret in text or srv.client_id in text,
                                    "markers": [m for m in mode.get("toolMarkers") or [] if m in text]}
        meta = self.headers.get("x-codex-turn-metadata")
        if responses and meta is not None and isinstance(sent, dict) and "instructions" in sent and "tools" in sent:
            record["harness"] = True
            try:
                record["sandboxMode"] = json.loads(meta).get("sandbox_mode")
            except ValueError:
                record["sandboxMode"] = None
        chat = mode.get("chat") or "text"
        with srv.lock:
            srv.serial += 1
            serial = srv.serial
        if chat.startswith("status:"):
            record["status"] = int(chat[7:])
            with srv.lock, open(srv.log, "a") as f:
                f.write(json.dumps(record) + "\n")
            self.respond(int(chat[7:]), json.dumps({"error": {"message": "synthetic refusal"}}).encode())
            return
        record["status"] = 200
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        cid = f"chatcmpl-{serial}"
        tool = mode.get("harnessTool")
        wanted = mode.get("harnessMcp")
        if responses and record.get("harness") and isinstance(wanted, dict) and "toolOutput" not in record:
            # C22: the model calls a workspace MCP tool as the engine offers it.
            for t in sent.get("tools") or []:
                name = t.get("name") or ""
                if wanted["server"] not in name:
                    continue
                if t.get("type") == "function" and name.endswith(wanted["tool"]):
                    tool = {"name": name, "arguments": wanted["arguments"]}
                elif t.get("type") == "namespace" and any(c.get("name") == wanted["tool"] for c in t.get("tools") or []):
                    tool = {"namespace": name, "name": wanted["tool"], "arguments": wanted["arguments"]}
            record["mcpOffered"] = isinstance(tool, dict) and tool is not mode.get("harnessTool")
        if responses and record.get("harness") and isinstance(tool, dict) and "toolOutput" not in record:
            self.harness_tool(f"resp_{serial}", tool)
            return
        if responses and record.get("harness"):
            self.harness_stream(f"resp_{serial}", record, mode.get("harness") or "short", serial)
            return
        if responses:
            self.response_stream(f"resp_{serial}", record, end=mode.get("responses") or "completed")
            return

        def chunk(text, finish=None, usage=None):
            event = {"id": cid, "object": "chat.completion.chunk", "created": 1760000000, "model": "gpt-4o",
                     "choices": [{"index": 0, "delta": {"content": text} if text is not None else {}, "finish_reason": finish}]}
            if usage:
                event["usage"] = usage
            return f"data: {json.dumps(event)}\n\n".encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True
        try:
            if chat == "hang":
                self.wfile.write(chunk("Synthetic partial answer"))
                self.wfile.flush()
                # Nothing more until the client goes away.
                while self.rfile.read(1):
                    pass
                raise ConnectionResetError
            if chat in ("bulk", "paced"):
                text = ("C18-STREAM-" + "x" * 32)[:32]
                for n in range(2048):
                    self.wfile.write(chunk(text))
                    if chat == "paced":
                        self.wfile.flush()
                        time.sleep(0.01)
            else:
                for text in ("Hello from the", " synthetic Gateway."):
                    self.wfile.write(chunk(text))
                    self.wfile.flush()
            self.wfile.write(chunk(None, "stop", {"prompt_tokens": 7, "completion_tokens": 12, "total_tokens": 19}))
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            with srv.lock, open(srv.log, "a") as f:
                f.write(json.dumps({"t": time.time(), "method": "POST", "path": "/rt/v1/chat/completions",
                                    "closedByClient": True, "trace": record["trace"]}) + "\n")

    def response_stream(self, rid, record, end="completed"):
        """A Responses stream: created, two text deltas, completed (or one
        delta, then incomplete at the output token limit, or failed)."""
        srv = self.server
        def response(status, usage):
            r = {"id": rid, "object": "response", "created_at": 1760000000, "status": status, "model": "gpt-4o-2024",
                 "metadata": None, "temperature": None, "top_p": None, "instructions": None, "tools": [],
                 "tool_choice": "auto", "error": None, "incomplete_details": None, "output": [],
                 "parallel_tool_calls": False}
            if usage:
                r["usage"] = {"input_tokens": 5, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 9,
                              "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 14}
            if status == "incomplete":
                r["incomplete_details"] = {"reason": "max_output_tokens"}
            return r

        def event(value):
            return f"data: {json.dumps(value)}\n\n".encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True
        try:
            self.wfile.write(event({"type": "response.created", "response": response("in_progress", False)}))
            for text in (("Partial from the",) if end != "completed" else ("Responses from the", " synthetic Gateway.")):
                self.wfile.write(event({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0,
                                        "content_index": 0, "delta": text}))
                self.wfile.flush()
            if end == "incomplete":
                self.wfile.write(event({"type": "response.incomplete", "response": response("incomplete", True)}))
            elif end == "failed":
                self.wfile.write(event({"type": "response.failed", "response": response("failed", False)}))
            else:
                self.wfile.write(event({"type": "response.completed", "response": response("completed", True)}))
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            with srv.lock, open(srv.log, "a") as f:
                f.write(json.dumps({"t": time.time(), "method": "POST", "path": "/rt/v1/responses",
                                    "closedByClient": True, "trace": record["trace"]}) + "\n")

    def harness_stream(self, rid, record, pace, serial):
        """C19: a Responses stream the native engine accepts."""
        srv = self.server

        def event(obj):
            return f"event: {obj['type']}\ndata: {json.dumps(obj)}\n\n".encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True
        base = {"id": rid, "object": "response", "status": "in_progress", "model": "fixture", "output": []}
        item = {"id": "msg_1", "type": "message", "role": "assistant", "status": "in_progress", "content": []}
        try:
            self.wfile.write(event({"type": "response.created", "response": base}))
            self.wfile.write(event({"type": "response.output_item.added", "output_index": 0, "item": item}))
            self.wfile.flush()
            # Numbered, so each answer is told apart from earlier ones.
            pieces = ([f"Answer {serial}: Hello from ", "the synthetic Gateway."] if pace == "short"
                      else ["thirty-two bytes of paced text. "] * 400 if pace == "paced"
                      # C20 "kib": a 1 KiB answer in 32-byte deltas.
                      else [f"Answer {serial}: Hello from the synthetic Gateway. ".ljust(64, ".")]
                      + ["thirty-two bytes of a long answer"] * 30)
            for piece in pieces:
                self.wfile.write(event({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0,
                                        "content_index": 0, "delta": piece}))
                self.wfile.flush()
                if pace == "paced":
                    time.sleep(0.05)
            text = "".join(pieces)
            done = dict(item, status="completed", content=[{"type": "output_text", "text": text, "annotations": []}])
            self.wfile.write(event({"type": "response.output_item.done", "output_index": 0, "item": done}))
            self.wfile.write(event({"type": "response.completed", "response": dict(base, status="completed", output=[done],
                usage={"input_tokens": 10, "output_tokens": 6, "total_tokens": 16,
                       "input_tokens_details": {"cached_tokens": 0}, "output_tokens_details": {"reasoning_tokens": 0}})}))
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            with srv.lock, open(srv.log, "a") as f:
                f.write(json.dumps({"t": time.time(), "method": "POST", "path": "/rt/v1/responses",
                                    "closedByClient": True, "harness": True}) + "\n")

    def harness_tool(self, rid, tool):
        """C21: the model asks for one tool call (mode "harnessTool":
        namespace, name and arguments as JSON text); the follow-up request
        that carries its output is answered as text."""
        def event(obj):
            return f"event: {obj['type']}\ndata: {json.dumps(obj)}\n\n".encode()
        item = {"type": "function_call", "call_id": f"call-{rid}", "name": tool["name"], "arguments": tool["arguments"]}
        if tool.get("namespace"):
            item["namespace"] = tool["namespace"]
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True
        try:
            self.wfile.write(event({"type": "response.created", "response": {"id": rid}}))
            self.wfile.write(event({"type": "response.output_item.done", "output_index": 0, "item": item}))
            self.wfile.write(event({"type": "response.completed", "response": {"id": rid,
                "usage": {"input_tokens": 1, "input_tokens_details": None, "output_tokens": 1,
                          "output_tokens_details": None, "total_tokens": 2}}}))
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def files(self, method, path, body, record, mode):
        """C18: the workspace's files with the inference key: list, one
        multipart upload, one delete. The content is hashed, never logged."""
        srv = self.server
        key = self.headers.get("x-portkey-api-key") or ""
        record["inferenceKeySha256"] = hashlib.sha256(key.encode()).hexdigest() if key else None
        record["trace"] = self.headers.get("x-portkey-trace-id")
        record["route"] = self.headers.get("x-portkey-config")
        reply = mode.get("files") or "ok"
        if method == "POST":
            parts = {p["name"]: p for p in multipart(self.headers.get("content-type") or "", body)}
            upload = parts.get("file") or {}
            content = upload.get("body") or b""
            record["filename"] = upload.get("filename")
            record["purpose"] = (parts.get("purpose") or {}).get("body", b"").decode("utf-8", "replace")
            record["bytes"] = len(content)
            record["contentSha256"] = hashlib.sha256(content).hexdigest()
        if method == "DELETE":
            record["file"] = path.rsplit("/", 1)[-1]
        status, payload = 200, None
        if method == "GET" and path == "/rt/v1/files":
            with srv.lock:
                payload = {"object": "list", "data": list(srv.files)}
            if mode.get("filesLate"):
                # A time later than any date can render (seconds).
                payload["data"].append({"id": "file-late", "object": "file", "bytes": 5, "created_at": 9000000000000,
                                        "filename": "late-notes.txt", "purpose": "assistants", "status": "processed"})
        elif reply.startswith("status:"):
            status, payload = int(reply[7:]), {"error": {"message": "synthetic refusal"}}
        elif method == "POST" and path == "/rt/v1/files":
            with srv.lock:
                srv.serial += 1
                created = {"id": f"file-up{srv.serial}", "object": "file", "bytes": record["bytes"],
                           "created_at": 1760000000, "filename": record["filename"], "purpose": "assistants",
                           "status": "processed"}
                srv.files.insert(0, created)
            payload = created
        elif method == "DELETE" and reply == "another":
            # Answered for another file: nothing is said about this one.
            payload = {"id": "file-other", "object": "file", "deleted": True}
        elif method == "DELETE":
            with srv.lock:
                srv.files = [f for f in srv.files if f["id"] != record["file"]]
            payload = {"id": record["file"], "object": "file", "deleted": True}
        else:
            status, payload = 404, {"error": {"message": "not here"}}
        record["status"] = "lost" if reply == "lost" and method != "GET" else status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        if record["status"] == "lost":
            self.close_connection = True
            return
        self.respond(status, json.dumps(payload).encode())

    def idp(self, method, path, query, body, record, mode):
        """C20: the synthetic OpenID provider (see the module docstring)."""
        srv = self.server
        issuer = f"https://localhost:{srv.server_address[1]}/idp"
        record["idp"] = path
        status, payload, location = 404, {"error": "not_found"}, None
        if path == "/.well-known/openid-configuration":
            status, payload = 200, {
                "issuer": issuer,
                "authorization_endpoint": f"{issuer}/authorize",
                "token_endpoint": f"{issuer}/token",
                "jwks_uri": f"{issuer}/jwks",
                "revocation_endpoint": f"{issuer}/revoke",
                "response_types_supported": ["code"],
                "code_challenge_methods_supported": ["S256"],
                "scopes_supported": ["openid", "offline_access"],
                "authorization_response_iss_parameter_supported": True,
            }
        elif path == "/jwks":
            status, payload = 200, {"keys": [srv.jwk]}
        elif path == "/authorize" and method == "GET":
            redirect = query.get("redirect_uri", "")
            ok = (query.get("response_type") == "code" and query.get("code_challenge_method") == "S256"
                  and redirect.startswith("http://127.0.0.1:") and query.get("state") and query.get("nonce"))
            if ok:
                with srv.lock:
                    srv.counter += 1
                    code = f"c20-code-{srv.counter}"
                    srv.idp_codes[code] = (query.get("code_challenge"), query.get("nonce"), redirect,
                                           query.get("client_id"))
                location = redirect + "?" + urlencode({"code": code, "state": query["state"], "iss": issuer})
                status, payload = 302, None
            else:
                status, payload = 400, {"error": "invalid_request"}
        elif path == "/token" and method == "POST":
            form = {k: v[0] for k, v in parse_qs(body.decode("utf-8", "replace")).items()}
            grant = form.get("grant_type")
            record["grantType"] = grant
            subject = mode.get("idpSubject") or "native-c20-alice"
            lifetime = int(mode.get("idpLifetime") or 3600)
            nonce, client = None, form.get("client_id")
            if grant == "authorization_code":
                with srv.lock:
                    bound = srv.idp_codes.pop(form.get("code", ""), None)
                verifier = form.get("code_verifier", "")
                challenge = b64url(hashlib.sha256(verifier.encode()).digest())
                if not bound or bound[0] != challenge or bound[2] != form.get("redirect_uri") or bound[3] != client:
                    status, payload = 400, {"error": "invalid_grant"}
                else:
                    nonce = bound[1]
                    status = 200
            elif grant == "refresh_token":
                with srv.lock:
                    known = srv.idp_refresh.pop(form.get("refresh_token", ""), None)
                    srv.keep_idp_refresh()
                if mode.get("idpRefresh") == "refuse" or known is None:
                    status, payload = 400, {"error": "invalid_grant"}
                else:
                    subject, status = known, 200
            else:
                status, payload = 400, {"error": "unsupported_grant_type"}
            if status == 200:
                now = int(time.time())
                access = srv.sign({"iss": issuer, "aud": [IDP_AUDIENCE], "azp": client, "sub": subject,
                                   "iat": now, "exp": now + lifetime, "portkey_workspace": "team-a"})
                with srv.lock:
                    srv.counter += 1
                    refresh = f"c20-refresh-{srv.counter}"
                    srv.idp_refresh[refresh] = subject
                    srv.keep_idp_refresh()
                    srv.idp_access.add(access)
                payload = {"token_type": "Bearer", "access_token": access, "expires_in": lifetime,
                           "refresh_token": refresh}
                if nonce is not None:
                    payload["id_token"] = srv.sign({"iss": issuer, "aud": client, "sub": subject, "iat": now,
                                                    "exp": now + 300, "nonce": nonce,
                                                    "preferred_username": f"{subject}@example.com"})
                record["subject"] = subject
                record["accessSha256"] = hashlib.sha256(access.encode()).hexdigest()
        elif path == "/revoke" and method == "POST":
            form = {k: v[0] for k, v in parse_qs(body.decode("utf-8", "replace")).items()}
            token = form.get("token", "")
            with srv.lock:
                srv.idp_refresh.pop(token, None)
                srv.keep_idp_refresh()
            record["revokedSha256"] = hashlib.sha256(token.encode()).hexdigest()
            status, payload = 200, {}
        record["status"] = status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        if location is not None:
            self.send_response(302)
            self.send_header("location", location)
            self.send_header("content-length", "0")
            self.send_header("connection", "close")
            self.end_headers()
            self.close_connection = True
            return
        self.respond(status, json.dumps(payload).encode())

    MCP_SERVERS = (("srv-tools-0001", "tools"), ("srv-docs-0001", "docs"))

    def mcp(self, method, path, query, body, record, mode):
        """C22: the Gateway's MCP plane (see the module docstring). Tokens
        are never logged: a request's bearer is recorded as issued,
        placeholder, other or none."""
        srv = self.server
        origin = f"https://localhost:{srv.server_address[1]}"
        issuer = f"{origin}/mcp-oauth"
        url_of = lambda slug: f"{origin}/mcp/{slug}/mcp"  # noqa: E731
        status, payload, location, headers = 404, {"error": "not_found"}, None, {}
        record["mcpPlane"] = path
        if path == "/ai_gw/v2/mcp-servers":
            if mode.get("mcpCatalog") == "down":
                status, payload = 500, {"error": {"message": "down"}}
            else:
                updated = mode.get("mcpUpdated") or "2026-10-01T00:00:00Z"
                tag = f" {mode['mcpTag']}" if mode.get("mcpTag") else ""
                rows = [{"id": sid, "name": f"{slug.title()} tools{tag}", "slug": slug, "url": url_of(slug),
                         "status": "active", "workspace_id": GW_A, "last_updated_at": updated,
                         "mcp_integration_url": "https://upstream.example/never-kept", "object": "mcp_server"}
                        for sid, slug in self.MCP_SERVERS if query.get("workspace_id") == GW_A]
                status, payload = 200, {"object": "list", "total": len(rows), "data": rows}
        elif path.startswith("/.well-known/oauth-protected-resource/mcp/") and path.endswith("/mcp"):
            slug = path[len("/.well-known/oauth-protected-resource/mcp/"):-len("/mcp")]
            status, payload = 200, {"resource": url_of(slug), "authorization_servers": [issuer],
                                    "bearer_methods_supported": ["header"]}
        elif path == "/.well-known/oauth-authorization-server/mcp-oauth":
            status, payload = 200, {
                "issuer": issuer, "authorization_endpoint": f"{issuer}/authorize",
                "token_endpoint": f"{issuer}/token", "registration_endpoint": f"{issuer}/register",
                "revocation_endpoint": f"{issuer}/revoke", "response_types_supported": ["code"],
                "grant_types_supported": ["authorization_code", "refresh_token"],
                "code_challenge_methods_supported": ["S256"], "token_endpoint_auth_methods_supported": ["none"],
                "scopes_supported": ["mcp:servers:read", "mcp:tools:list", "mcp:tools:call", "mcp:*"]}
        elif path == "/mcp-oauth/register" and method == "POST":
            sent = json.loads(body or b"{}")
            with srv.lock:
                srv.counter += 1
                client = f"c22-client-{srv.counter}"
                srv.mcp_clients[client] = (sent.get("redirect_uris") or [""])[0]
            record["registered"] = sent.get("token_endpoint_auth_method") == "none"
            status, payload = 201, {"client_id": client}
        elif path == "/mcp-oauth/authorize" and method == "GET":
            redirect = query.get("redirect_uri", "")
            with srv.lock:
                known = srv.mcp_clients.get(query.get("client_id", "")) == redirect
            record["scope"] = query.get("scope")
            record["resource"] = query.get("resource")
            if known and query.get("code_challenge_method") == "S256" and query.get("state"):
                with srv.lock:
                    srv.counter += 1
                    code = f"c22-code-{srv.counter}"
                    srv.mcp_codes[code] = (query.get("code_challenge"), redirect, query.get("client_id"),
                                           query.get("resource"))
                location = redirect + "?" + urlencode({"code": code, "state": query["state"]})
                status, payload = 302, None
            else:
                status, payload = 400, {"error": "invalid_request"}
        elif path == "/mcp-oauth/token" and method == "POST":
            form = {k: v[0] for k, v in parse_qs(body.decode("utf-8", "replace")).items()}
            grant = form.get("grant_type")
            record["grantType"] = grant
            bound = None
            if grant == "authorization_code":
                with srv.lock:
                    code = srv.mcp_codes.pop(form.get("code", ""), None)
                challenge = b64url(hashlib.sha256(form.get("code_verifier", "").encode()).digest())
                if code and code[0] == challenge and code[1] == form.get("redirect_uri") \
                        and code[2] == form.get("client_id") and code[3] == form.get("resource"):
                    bound = (code[3], code[2])
            elif grant == "refresh_token":
                with srv.lock:
                    known = srv.mcp_refresh.pop(form.get("refresh_token", ""), None)
                if known and mode.get("mcpRefresh") != "refuse" and known == (form.get("resource"), form.get("client_id")):
                    bound = known
            if bound is None:
                status, payload = 400, {"error": "invalid_grant"}
            else:
                lifetime = int(mode.get("mcpLifetime") or 3600)
                with srv.lock:
                    srv.counter += 1
                    access = f"c22-mcp-access-{srv.counter}-{secrets_token()}"
                    refresh = f"c22-mcp-refresh-{srv.counter}-{secrets_token()}"
                    srv.mcp_access[access] = bound
                    srv.mcp_refresh[refresh] = bound
                status, payload = 200, {"token_type": "Bearer", "access_token": access, "expires_in": lifetime,
                                        "refresh_token": refresh}
        elif path == "/mcp-oauth/revoke" and method == "POST":
            form = {k: v[0] for k, v in parse_qs(body.decode("utf-8", "replace")).items()}
            with srv.lock:
                srv.mcp_access.pop(form.get("token", ""), None)
                srv.mcp_refresh.pop(form.get("token", ""), None)
            record["revokedHint"] = form.get("token_type_hint")
            status, payload = 200, {}
        elif path.startswith("/mcp/") and path.endswith("/mcp"):
            slug = path[len("/mcp/"):-len("/mcp")]
            header = self.headers.get("authorization") or ""
            token = header[7:] if header.startswith("Bearer ") else None
            with srv.lock:
                granted = token is not None and srv.mcp_access.get(token, (None,))[0] == url_of(slug)
            record["mcp"] = slug
            record["mcpBearer"] = ("issued" if granted else "placeholder" if token == "airs-desktop-mcp-sign-in-expired"
                                   else "other" if token else "none")
            if not granted:
                headers["www-authenticate"] = (f'Bearer error="invalid_token", resource_metadata='
                                               f'"{origin}/.well-known/oauth-protected-resource/mcp/{slug}/mcp"')
                status, payload = 401, {"error": "unauthorized"}
            elif method != "POST":
                status, payload = 405, {}
            else:
                sent = json.loads(body or b"{}")
                if "id" not in sent:
                    status, payload = 202, None
                else:
                    name = sent.get("method")
                    record["mcpMethod"] = name
                    if name == "initialize":
                        result = {"protocolVersion": (sent.get("params") or {}).get("protocolVersion"),
                                  "capabilities": {"tools": {}}, "serverInfo": {"name": slug, "version": "1.0.0"}}
                    elif name == "tools/list":
                        result = {"tools": [{"name": "lookup", "description": "Look up a workspace record",
                                             "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}},
                                                             "required": ["query"]}}]}
                    elif name == "tools/call":
                        record["mcpCall"] = (sent.get("params") or {}).get("name")
                        result = {"content": [{"type": "text", "text": "C22-NATIVE-MCP-RESULT"}], "isError": False}
                    else:
                        result = {}
                    status, payload = 200, {"jsonrpc": "2.0", "id": sent["id"], "result": result}
        record["status"] = status
        with srv.lock, open(srv.log, "a") as f:
            f.write(json.dumps(record) + "\n")
        if location is not None:
            self.send_response(302)
            self.send_header("location", location)
            self.send_header("content-length", "0")
            self.send_header("connection", "close")
            self.end_headers()
            self.close_connection = True
            return
        if payload is None:
            self.send_response(status)
            self.send_header("content-length", "0")
            self.send_header("connection", "close")
            self.end_headers()
            self.close_connection = True
            return
        data = json.dumps(payload).encode()
        self.send_response(status)
        for k, v in headers.items():
            self.send_header(k, v)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True
        self.wfile.write(data)

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


IDP_AUDIENCE = "https://gateway.example/native-c20"


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_signature_to_raw(der):
    """An ECDSA DER signature (SEQUENCE of two INTEGERs) as JWS r || s."""
    def integer(at):
        assert der[at] == 0x02
        n = der[at + 1]
        value = der[at + 2:at + 2 + n].lstrip(b"\x00")
        return value.rjust(32, b"\x00"), at + 2 + n
    at = 2 if der[1] < 0x80 else 3
    r, at = integer(at)
    s, _ = integer(at)
    return r + s


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
        # C20: the provider's signing key, made for this run.
        self.idp_codes, self.idp_refresh, self.idp_access = {}, {}, set()
        # Refresh tokens and the signing key are kept across a restart
        # (offline and back, T23), like the provider.
        try:
            with open(os.path.join(directory, "idp-refresh.json")) as f:
                self.idp_refresh = dict(json.load(f))
        except (OSError, ValueError):
            pass
        # C22: the MCP plane's OAuth clients, codes and tokens.
        self.mcp_clients, self.mcp_codes, self.mcp_access, self.mcp_refresh = {}, {}, {}, {}
        self.idp_key = os.path.join(directory, "idp-key.pem")
        if not os.path.exists(self.idp_key):
            subprocess.run(["/usr/bin/openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout",
                            "-out", self.idp_key], check=True, env={}, capture_output=True)
        public = subprocess.run(["/usr/bin/openssl", "ec", "-in", self.idp_key, "-pubout", "-outform", "DER"],
                                check=True, env={}, capture_output=True).stdout[-65:]
        self.jwk = {"kty": "EC", "crv": "P-256", "kid": "native-c20", "use": "sig", "alg": "ES256",
                    "x": b64url(public[1:33]), "y": b64url(public[33:65])}
        self.serial = 0
        self.files = [{"id": "file-seed", "object": "file", "bytes": 64, "created_at": 1760000000,
                       "filename": "seed-notes.txt", "purpose": "assistants", "status": "processed"}]
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
        self.gw = {"renamed": {}, "deleted": set(), "created": {"api-keys/service": []}, "scopes": {},
                   "workspaces": [], "archived": set()}
        self.topics = [{"topic_id": "11111111-1111-4111-8111-111111111111", "topic_name": "Finance",
                        "revision": 1, "description": "Money talk"}]
        self.apps = [{"customer_appId": "app-1", "tsg_id": tsg, "app_name": "Support bot",
                      "cloud_provider": "aws", "environment": "production"}]
        self.keys = [{"api_key_id": "key-1", "api_key_name": "Runtime", "api_key_last8": "abcd1234",
                      "api_key": "SYNTHETIC-C08-KEY-VALUE", "auth_code": "SYNTHETIC-C08-AUTH-CODE", "cust_app": "Support bot",
                      "expiration": "2027-01-01T00:00:00Z", "revoked": False, "status": "FUTURE_STATUS"}]

    def sign(self, claims):
        header = b64url(json.dumps({"alg": "ES256", "typ": "JWT", "kid": "native-c20"}).encode())
        payload = b64url(json.dumps(claims).encode())
        signing_input = f"{header}.{payload}".encode()
        der = subprocess.run(["/usr/bin/openssl", "dgst", "-sha256", "-sign", self.idp_key],
                             input=signing_input, check=True, env={}, capture_output=True).stdout
        return f"{header}.{payload}.{b64url(der_signature_to_raw(der))}"

    def keep_idp_refresh(self):
        """Writes the provider's live refresh tokens (called under the lock)."""
        with open(os.path.join(self.directory, "idp-refresh.json"), "w") as f:
            json.dump(self.idp_refresh, f)

    def mode(self):
        try:
            with open(os.path.join(self.directory, "mode.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}


def secrets_token():
    return base64.urlsafe_b64encode(os.urandom(12)).decode().rstrip("=")


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
