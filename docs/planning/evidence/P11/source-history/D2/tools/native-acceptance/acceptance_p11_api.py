"""C48-N01..03 source for the P11 single-writer native orchestrator.

No work runs at import time. All requests target this module's owned
127.0.0.1 fixture. Clipboard comparisons begin with driver-written text;
unexpected clipboard contents are never logged. Native sheets are inspected
in memory only, never captured while they may list the operator's folders.
Images are required review inputs, not automatic visual passes. No AX range
or third-party input-method qualification is claimed.
"""

import hashlib
import json
from pathlib import Path
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


SOURCE_REQUIREMENTS = {
    "frontend": [
        "src/v2/workbench/api-client.test.tsx",
        "src/v2/workbench/request-presentation.test.ts",
        "src/modules/workbench/document-save.test.tsx",
        "src/v2/workbench/workbench.test.tsx",
    ],
    "rust": [
        "workbench_documents",
        "refinement_startup_",
        "workbench_execution",
        "refinement_response_suggestions_",
        "workbench::profile::tests::an_existing_profile_has_an_uninitialized_collection_choice",
    ],
    "not_claimed_by_native": [
        "exhaustive asynchronous bridge ordering and previous-collection isolation",
        "startup preference migration and all private-profile failure cases",
        "byte preservation for every payload, limit and save-conflict case",
        "AX selection offsets or general third-party input-method qualification",
        "visual token, selection, caret or scrolling correctness without root capture review",
    ],
}


BODY = '{"integer":9007199254740993,"escaped":"\\u0061\\/","text":"a b"}\n'
RESPONSE = BODY.encode("utf-8")
TRUNCATED = b'{"value":"' + b"x" * (256 * 1024) + b'"}\n'
PREVIEW_BYTES = 256 * 1024
MAIN = "My request.request.toml"
COLLECTION_NAME = "P11 API"
COLLECTION_ID = "p11-api-owned"
TAB_NAMES = ["tab-%02d" % n for n in range(1, 11)]
REQUEST_COUNT = 6 + len(TAB_NAMES)
ROLES = ("AXButton", "AXPopUpButton", "AXMenuItem", "AXRadioButton", "AXTab", "AXCheckBox")


class _Endpoint:
    """Only synthetic bodies/digests; no authorization/header values kept."""

    def __init__(self):
        self.lock = threading.Lock()
        self.rows = []
        self.gates = {"/slow": threading.Event(), "/hang": threading.Event()}
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def handle_request(self):
                length = int(self.headers.get("content-length", "0"))
                if length < 0 or length > 1024 * 1024:
                    self.send_error(413)
                    return
                data = self.rfile.read(length)
                path = urlsplit(self.path).path
                with owner.lock:
                    row = {"path": path, "method": self.command, "bytes": len(data),
                           "sha256": hashlib.sha256(data).hexdigest(),
                           "target_sha256": hashlib.sha256(self.path.encode()).hexdigest(),
                           "authorization_absent": self.headers.get("authorization") is None,
                           "response_attempted": False}
                    owner.rows.append(row)
                    gate = owner.gates.get(path)
                if gate is not None and not gate.wait(30):
                    return
                status = 201 if path == "/fast" else 200
                response = (TRUNCATED if path == "/truncated" else b'{"unfinished":' if path == "/invalid"
                            else b'{"native":"fast"}\n' if path == "/fast"
                            else b'{"native":"slow"}\n' if path == "/slow" else RESPONSE)
                try:
                    self.send_response(status)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(response)))
                    self.send_header("connection", "close")
                    self.end_headers()
                    self.wfile.write(response)
                except (BrokenPipeError, ConnectionResetError):
                    pass  # Retired/cancelled clients are expected in N02.
                finally:
                    with owner.lock:
                        row["response_attempted"] = True

            do_GET = do_POST = handle_request

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=lambda: self.server.serve_forever(poll_interval=0.1), daemon=True)
        self.thread.start()
        self.origin = "http://127.0.0.1:%d" % self.server.server_address[1]

    def requests(self, path=None):
        with self.lock:
            return [dict(row) for row in self.rows if path is None or row["path"] == path]

    def hold(self, path):
        with self.lock:
            self.gates[path] = threading.Event()

    def release(self, path):
        with self.lock:
            self.gates[path].set()

    def stop(self):
        with self.lock:
            for gate in self.gates.values():
                gate.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)


def _press(ctx, label):
    tree, found = ctx.app.wait_for(lambda t: any(n.get("role") in ROLES and n.get("label") == label for n in t), 12)
    ctx.require(found, "C48 control is absent: " + label)
    node = next(n for n in tree if n.get("role") in ROLES and n.get("label") == label)
    return ctx.press(node["role"], label)


def _input(ctx, label):
    tree, found = ctx.app.wait_for(lambda t: any(n.get("role") in ("AXTextField", "AXTextArea") and n.get("label") == label for n in t), 12)
    ctx.require(found, "C48 input is absent: " + label)
    nodes = [n for n in tree if n.get("role") in ("AXTextField", "AXTextArea") and n.get("label") == label]
    ctx.require(len(nodes) == 1 and nodes[0].get("subrole") != "AXSecureTextField", "C48 input must be unique and nonsecure")
    return nodes[0]


def _focus(ctx, label):
    node = _input(ctx, label)
    ctx.app.ax("focus", node["role"], label)
    tree, found = ctx.app.wait_for(lambda t: any(n.get("focused") and n.get("role") == node["role"] and n.get("label") == label for n in t), 5)
    ctx.require(found, "C48 input did not receive native focus")
    return tree


def _clipboard_write(text):
    subprocess.run(["/usr/bin/pbcopy"], input=text.encode("utf-8"), check=True)


def _paste(ctx, label, text):
    _clipboard_write(text)
    _focus(ctx, label)
    ctx.app.keys("a", "command down")
    ctx.app.keys("v", "command down")
    ctx.require(_input(ctx, label).get("value") == text, "Owned C48 paste changed its text")


def _copy_matches(ctx, expected):
    # Replacing the pasteboard before Copy means a failed Copy cannot cause
    # a read of the operator's preexisting clipboard. Never print the result.
    _clipboard_write("P11-owned-copy-sentinel")
    ctx.app.keys("c", "command down")
    actual = subprocess.check_output(["/usr/bin/pbpaste"])
    return actual == expected.encode("utf-8")


def _status_regions(tree):
    regions = []
    for i, node in enumerate(tree):
        if node.get("subrole") != "AXApplicationStatus":
            continue
        values = []
        for child in tree[i + 1:]:
            if child.get("depth", 0) <= node.get("depth", 0):
                break
            values.append(str(child.get("label") or child.get("value") or ""))
        regions.append((node, " ".join(values)))
    return regions


def _metrics(ctx, name, code, history=False):
    def matches(tree):
        for node, text in _status_regions(tree):
            if name + " ·" not in text or ("History" in text) != history:
                continue
            start = tree.index(node)
            for child in tree[start + 1:]:
                if child.get("depth", 0) <= node.get("depth", 0):
                    break
                if (child.get("label") or child.get("value")) in (str(code), str(code) + " OK"):
                    return True
        return False
    tree, found = ctx.app.wait_for(matches, 15)
    ctx.require(found, "C48 displayed-response footer does not name the expected record")
    return tree


def _workbench_count(data):
    # Only the disposable fixture's bounded, typed/redacted event log.
    total = 0
    root = data / "fixture-logs"
    for name in ("diagnostics.jsonl", "diagnostics.1.jsonl"):
        path = root / name
        if not path.exists():
            continue
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 256 * 1024:
            raise RuntimeError("C48 fixture diagnostics are not bounded regular files")
        for line in path.read_text().splitlines():
            event = json.loads(line).get("event", {})
            total += event.get("type") == "intent" and event.get("kind") == "workbench" and event.get("refusal") is None
    return total


def _quiet_count(ctx, data):
    last, stable = None, time.monotonic()
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        count = _workbench_count(data)
        if count != last:
            last, stable = count, time.monotonic()
        elif time.monotonic() - stable >= 0.8:
            return count
        time.sleep(0.1)
    raise RuntimeError("C48 workbench did not become quiescent before Save")


def _save_once(ctx, data):
    before = _quiet_count(ctx, data)
    ctx.app.keys("s", "command down")
    tree, saved = ctx.app.wait_for(lambda t: any(text.strip() == "Saved" for _n, text in _status_regions(t)), 12)
    after = _quiet_count(ctx, data)
    return saved and after - before == 1, tree


def _request(root, filename, identity, method, url, body=False):
    text = 'format_version = 1\nid = %s\nmethod = %s\nurl = %s\n' % (json.dumps(identity), json.dumps(method), json.dumps(url))
    if body:
        text += 'body = "owned-body.json"\nbody_kind = "json"\n'
    (root / filename).write_text(text)


def _stage(root, origin, replacement=False):
    root.mkdir()
    name = "P11 Replacement" if replacement else COLLECTION_NAME
    (root / "collection.toml").write_text('format_version = 1\nid = %s\nname = %s\n\n[variables]\nbase = %s\ntag = "owned"\n' % (json.dumps(COLLECTION_ID), json.dumps(name), json.dumps(origin)))
    if replacement:
        _request(root, "fast.request.toml", "fast", "GET", "{{base}}/fast?collection=replacement")
        return
    (root / "owned-body.json").write_text(BODY)
    _request(root, MAIN, "owned-main", "POST", "{{base}}/json?tag={{tag}}", True)
    for name in ("invalid", "truncated", "slow", "fast", "hang"):
        _request(root, name + ".request.toml", name, "GET", "{{base}}/" + name)
    for name in TAB_NAMES:
        _request(root, name + ".request.toml", name, "GET", "{{base}}/json?tab=" + name)


def _close_collection(ctx, name):
    _press(ctx, "Collection " + name)
    _press(ctx, "Close collection")
    tree = ctx.text("No collection is open.")
    ctx.require(ctx.app.has_text(tree, "No collection is open."), "Collection did not close")


def _open_collection(ctx, data, root, name=COLLECTION_NAME):
    (data / "fixture-open-folder").write_text(str(root) + "\n")
    _press(ctx, "Open collection…")
    tree = ctx.text("Collection " + name)
    ctx.require(ctx.app.has_text(tree, "Collection " + name), "Owned collection did not open")
    return tree


def _open_request(ctx, filename, method="GET"):
    _press(ctx, "Open " + method + " " + filename)
    return _input(ctx, "URL")


def _json_body_tab(ctx):
    tree, found = ctx.app.wait_for(lambda t: any(n.get("role") in ("AXRadioButton", "AXTab") and (n.get("label") or "").startswith("Body") and "JSON" in (n.get("label") or "") for n in t), 12)
    ctx.require(found, "C48 request JSON Body tab is absent")
    node = next(n for n in tree if n.get("role") in ("AXRadioButton", "AXTab") and (n.get("label") or "").startswith("Body") and "JSON" in (n.get("label") or ""))
    ctx.press(node["role"], node["label"])


def _restart(ctx):
    ctx.app.quit()
    ctx.app.launch()
    ctx.navigate("API Client")


def _history(ctx, prefix):
    _press(ctx, "History")
    tree, found = ctx.app.wait_for(lambda t: any(n.get("role") == "AXButton" and (n.get("label") or "").startswith(prefix) for n in t), 12)
    ctx.require(found, "C48 owned history entry is absent")
    node = next(n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith(prefix))
    ctx.press("AXButton", node["label"])


def _visible(tree, node):
    strip = next((n for n in tree if n.get("label") == "Open requests" and n.get("role") in ("AXTabGroup", "AXGroup", "AXRadioGroup") and n.get("pos") and n.get("size")), None)
    if not strip or not node or not node.get("pos") or not node.get("size"):
        return False
    return node["pos"][0] >= strip["pos"][0] - 1 and node["pos"][0] + node["size"][0] <= strip["pos"][0] + strip["size"][0] + 1


def run(ctx):
    import fixture_app as fx
    import native_sheets as sheets

    app = ctx.app
    data = Path(fx.DATA)
    work = Path(ctx.work) / "c48-owned-api"
    ctx.require(not work.exists(), "C48 scratch must be a fresh owned directory")
    work.mkdir(mode=0o700)
    root, replacement = work / "collection", work / "replacement"
    endpoint = _Endpoint()
    native_sheets_before = Path(sheets.FLAG).exists()
    try:
        source_requirements = ctx.save("c48-source-boundary-requirements", SOURCE_REQUIREMENTS)
        ctx.state["c48_source_requirements"] = SOURCE_REQUIREMENTS
        sheets.disable()  # Ordinary folder opens use the fixture-only adapter.
        _stage(root, endpoint.origin)
        _stage(replacement, endpoint.origin, True)
        ctx.navigate("API Client")
        initial = ctx.text("193 requests")
        first_use = app.has_text(initial, "Collection Strata Cloud Manager") and app.has_text(initial, "193 requests")
        ctx.require(first_use, "First API use did not open the built-in SCM193")
        first_evidence = ctx.save("c48-first-use", initial)
        _close_collection(ctx, "Strata Cloud Manager")
        _open_collection(ctx, data, root)

        # N01: real WebKit native text/selection/caret/copy and actual Save.
        _open_request(ctx, MAIN, "POST")
        url = "{{base}}/json?tag={{tag}}"
        _focus(ctx, "URL")
        app.keycode(123, "command down")
        for _ in range(len("{{base}}")):
            app.keycode(124, "shift down")
        url_selected = _copy_matches(ctx, "{{base}}")
        url_capture = ctx.capture("c48-url-selected-token")
        app.keycode(124)
        app.keys("/caret")
        caret = _input(ctx, "URL").get("value") == "{{base}}/caret/json?tag={{tag}}"
        _paste(ctx, "URL", url + "&saved=once")
        save_url, _tree = _save_once(ctx, data)
        _paste(ctx, "URL", url)
        restored_url, _tree = _save_once(ctx, data)
        save_url = save_url and restored_url
        url_file_exact = 'url = ' + json.dumps(url) in (root / MAIN).read_text()
        _json_body_tab(ctx)
        _focus(ctx, "Body")
        app.keys("a", "command down")
        body_copy = _copy_matches(ctx, BODY)
        body_capture = ctx.capture("c48-json-body-selection")
        _paste(ctx, "Body", "x")
        app.keycode(124, "command down")
        app.keys("e", "option down")
        marked_capture = ctx.capture("c48-native-dead-key-marked")
        app.keys("e")
        composition = _input(ctx, "Body").get("value") == "xé"
        _paste(ctx, "Body", '{"unfinished":')
        invalid_edit = _input(ctx, "Body").get("value") == '{"unfinished":'
        _paste(ctx, "Body", BODY + " ")
        save_body, _tree = _save_once(ctx, data)
        body_file_exact = (root / "owned-body.json").read_bytes() == (BODY + " ").encode()
        _paste(ctx, "Body", BODY)
        restored_body, _tree = _save_once(ctx, data)
        save_body = save_body and restored_body
        body_file_exact = body_file_exact and (root / "owned-body.json").read_bytes() == RESPONSE
        _press(ctx, "Send")
        tree = _metrics(ctx, "My request", 200)
        main_sent = endpoint.requests("/json")
        input_exact = len(main_sent) == 1 and main_sent[0]["method"] == "POST" and main_sent[0]["sha256"] == hashlib.sha256(RESPONSE).hexdigest() and main_sent[0]["target_sha256"] == hashlib.sha256(b"/json?tag=owned").hexdigest()
        integer = app.has_text(tree, "9007199254740993") and app.has_text(tree, "\\u0061\\/")
        _clipboard_write("P11-owned-response-copy-sentinel")
        _press(ctx, "Copy")
        # App's Copy owns this pasteboard value: compare, never log it.
        raw_copy = subprocess.check_output(["/usr/bin/pbpaste"]) == RESPONSE
        pretty_capture = ctx.capture("c48-pretty-exact-tokens")
        _press(ctx, "Raw")
        raw_tree = ctx.save("c48-raw-response")
        _open_request(ctx, "invalid.request.toml")
        _press(ctx, "Send")
        tree = _metrics(ctx, "invalid", 200)
        invalid_preview = app.has_text(tree, '{"unfinished":')
        _open_request(ctx, "truncated.request.toml")
        _press(ctx, "Send")
        tree = _metrics(ctx, "truncated", 200)
        _clipboard_write("P11-owned-truncated-copy-sentinel")
        _press(ctx, "Copy")
        truncated_copy = subprocess.check_output(["/usr/bin/pbpaste"]) == TRUNCATED[:PREVIEW_BYTES]
        ctx.record("C48-N01", {"url_segment_copy_exact": url_selected, "caret_insertion_at_selection_end": caret,
                   "native_body_copy_exact": body_copy, "native_dead_key_composition_committed": composition,
                   "invalid_json_editable": invalid_edit, "url_save_admitted_once": save_url,
                   "body_save_admitted_once": save_body, "saved_url_template_exact": url_file_exact, "saved_body_bytes_exact": body_file_exact,
                   "sent_body_bytes_exact": input_exact, "pretty_large_integer_and_escape_exact": integer,
                   "response_copy_bytes_exact": raw_copy, "invalid_preview_kept": invalid_preview,
                   "truncated_copy_is_original_preview": truncated_copy},
                   [source_requirements, raw_tree, ctx.save("c48-invalid-truncated", tree)],
                   visual=[url_capture, body_capture, marked_capture, pretty_capture])

        # N02: one scroll row, focused/selected visibility, response identity.
        for name in TAB_NAMES:
            _open_request(ctx, name + ".request.toml")
        tree = app.tree()
        tabs = [n for n in tree if n.get("role") in ("AXRadioButton", "AXTab") and (n.get("label") or "").startswith("GET tab-")]
        single_row = len(tabs) == len(TAB_NAMES) and all(n.get("pos") for n in tabs) and max(n["pos"][1] for n in tabs) - min(n["pos"][1] for n in tabs) <= 3
        last = next((n for n in tabs if n.get("label") == "GET " + TAB_NAMES[-1]), None)
        selected_visible = _visible(tree, last)
        first = next((n for n in tabs if n.get("label") == "GET " + TAB_NAMES[0]), None)
        ctx.require(first is not None, "C48 first scrolling request tab is absent")
        app.ax("focus", first["role"], first["label"])
        tree = app.tree()
        focused_visible = _visible(tree, next((n for n in tree if n.get("role") == first["role"] and n.get("label") == first["label"]), None))
        tabs_capture = ctx.capture("c48-scrolling-tab-focus")
        _open_request(ctx, "slow.request.toml")
        _press(ctx, "Send")
        ctx.text("Cancel request")
        _open_request(ctx, "fast.request.toml")
        _press(ctx, "Send")
        _metrics(ctx, "fast", 201)
        endpoint.release("/slow")
        time.sleep(0.8)
        tree = _metrics(ctx, "fast", 201)
        late_kept = not any("slow ·" in text for _n, text in _status_regions(tree))
        _history(ctx, "GET slow · HTTP 200")
        tree = _metrics(ctx, "slow", 200, True)
        history_identity = any("History" in text and "slow ·" in text for _n, text in _status_regions(tree))
        history_capture = ctx.capture("c48-history-footer-identity")
        _open_request(ctx, "hang.request.toml")
        _press(ctx, "Send")
        ctx.text("Cancel request")
        _history(ctx, "GET slow · HTTP 200")
        tree = _metrics(ctx, "slow", 200, True)
        cancel_in_history = app.node(tree, "AXButton", "Cancel request") is not None
        _press(ctx, "Cancel request")
        ctx.text("Cancelled")
        endpoint.release("/hang")
        endpoint.hold("/slow")
        _open_request(ctx, "slow.request.toml")
        _press(ctx, "Send")
        ctx.text("Cancel request")
        _close_collection(ctx, COLLECTION_NAME)
        _open_collection(ctx, data, replacement, "P11 Replacement")
        _open_request(ctx, "fast.request.toml")
        _press(ctx, "Send")
        _metrics(ctx, "fast", 201)
        endpoint.release("/slow")
        time.sleep(0.8)
        tree = _metrics(ctx, "fast", 201)
        scoped = app.has_text(tree, "Collection P11 Replacement") and app.has_text(tree, "1 request") and not any("slow ·" in text for _n, text in _status_regions(tree))
        footer_capture = ctx.capture("c48-current-collection-footer")
        ctx.record("C48-N02", {"tabs_in_one_row": bool(single_row), "selected_tab_visible": selected_visible,
                   "focused_tab_visible": focused_visible, "late_background_result_preserves_current_footer": late_kept,
                   "history_footer_names_record": history_identity, "active_cancel_retained_in_history": cancel_in_history,
                   "replaced_collection_footer_is_scoped": scoped, "separate_save_state_present": app.has_text(tree, "Saved")},
                   [source_requirements, ctx.save("c48-footer-scoped", tree)], visual=[tabs_capture, history_capture, footer_capture])

        # N03: real response picker, exact bytes and persistent collection state.
        _close_collection(ctx, "P11 Replacement")
        _open_collection(ctx, data, root)
        _open_request(ctx, MAIN, "POST")
        _press(ctx, "Send")
        _metrics(ctx, "My request", 200)
        default_dir, chosen_dir = work / "default-response", work / "chosen directory with spaces"
        default_dir.mkdir()
        chosen_dir.mkdir()
        sheets.enable()
        _press(ctx, "Save response…")
        ctx.require(sheets.wait_sheet(app), "Native response Save panel did not open")
        # No persisted tree/image while a native panel may expose host names.
        panel = app.tree()
        suggested = any(n.get("role") == "AXTextField" and n.get("value") in ("My-request-response", "My-request-response.json") for n in panel)
        ctx.require(sheets.save_as(app, str(default_dir)), "Native default response Save failed")
        _press(ctx, "Save response…")
        ctx.require(sheets.save_as(app, str(chosen_dir), "My explicitly chosen name.json"), "Native chosen response Save failed")
        sheets.disable()
        default = default_dir / "My-request-response.json"
        chosen = chosen_dir / "My explicitly chosen name.json"
        default_exact = default.is_file() and default.read_bytes() == RESPONSE
        chosen_exact = chosen.is_file() and chosen.read_bytes() == RESPONSE
        picker_evidence = ctx.save("c48-response-saved")
        _close_collection(ctx, COLLECTION_NAME)
        _press(ctx, "Open Strata Cloud Manager")
        _restart(ctx)
        tree = ctx.text("193 requests")
        builtin_kept = app.has_text(tree, "Collection Strata Cloud Manager")
        _close_collection(ctx, "Strata Cloud Manager")
        _restart(ctx)
        tree = ctx.text("No collection is open.")
        closed_kept = app.has_text(tree, "No collection is open.") and not app.has_text(tree, "193 requests")
        _open_collection(ctx, data, root)
        before_restart = len(endpoint.requests())
        _restart(ctx)
        tree = ctx.text("Collection " + COLLECTION_NAME)
        custom_kept = app.has_text(tree, str(REQUEST_COUNT) + " requests")
        no_replay = len(endpoint.requests()) == before_restart
        app.quit()
        moved = work / "temporarily-missing"
        root.rename(moved)
        app.launch()
        ctx.navigate("API Client")
        tree = ctx.text("The preferred collection could not be opened.")
        missing = not root.exists() and app.has_text(tree, "Try again") and not app.has_text(tree, "193 requests")
        recovery_capture = ctx.capture("c48-missing-custom-recovery")
        recovery_tree = ctx.save("c48-missing-custom", tree)
        moved.rename(root)
        _press(ctx, "Try again")
        ctx.text("Collection " + COLLECTION_NAME)
        app.quit()
        manifest = root / "collection.toml"
        original_manifest = manifest.read_bytes()
        changed_manifest = original_manifest.replace(COLLECTION_ID.encode(), b"p11-replaced-id")
        manifest.write_bytes(changed_manifest)
        app.launch()
        ctx.navigate("API Client")
        tree = ctx.text("The preferred collection could not be opened.")
        replaced = manifest.read_bytes() == changed_manifest and not app.has_text(tree, "193 requests")
        manifest.write_bytes(original_manifest)
        _press(ctx, "Try again")
        ctx.text("Collection " + COLLECTION_NAME)
        app.quit()
        redirected = work / "redirected-owned-root"
        root.rename(redirected)
        root.symlink_to(redirected, target_is_directory=True)
        app.launch()
        ctx.navigate("API Client")
        tree = ctx.text("The preferred collection could not be opened.")
        refused_redirect = root.is_symlink() and root.resolve() == redirected.resolve() and not app.has_text(tree, "193 requests")
        root.unlink()
        redirected.rename(root)
        _press(ctx, "Try again")
        ctx.text("Collection " + COLLECTION_NAME)
        _open_request(ctx, MAIN, "POST")
        ctx.record("C48-N03", {"first_use_scm193": first_use, "builtin_choice_survives_restart": builtin_kept,
                   "explicit_closed_choice_survives_restart": closed_kept, "custom_choice_survives_restart": custom_kept,
                   "no_request_replayed_on_restart": no_replay, "missing_root_offers_recovery_without_creation_or_fallback": missing,
                   "replaced_identity_refused": replaced, "redirected_root_refused": refused_redirect,
                   "native_suggestion_hyphenated": suggested, "default_response_filename_and_bytes_exact": default_exact,
                   "explicit_filename_directory_and_bytes_exact": chosen_exact,
                   "all_endpoint_authorization_headers_absent": all(r["authorization_absent"] for r in endpoint.requests())},
                   [source_requirements, first_evidence, picker_evidence, recovery_tree, ctx.save("c48-recovered-custom")], visual=[recovery_capture])
        ctx.state["api_collection_root"] = str(root)
        ctx.state["api_main_request"] = MAIN
        ctx.navigate("Home")
    finally:
        # Close any owned native picker before the orchestrator captures a
        # failure tree; its default directory may otherwise reveal host names.
        if Path(sheets.FLAG).exists() and app.pid is not None and app.pid == app.running_pid():
            try:
                for _ in range(3):
                    if not sheets.sheets(app.tree()):
                        break
                    app.keycode(53)
            except RuntimeError:
                pass  # The orchestrator's exact-candidate cleanup remains authoritative.
        endpoint.stop()
        _clipboard_write("")
        if native_sheets_before:
            sheets.enable()
        else:
            sheets.disable()
