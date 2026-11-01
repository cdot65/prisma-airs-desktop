"""Shared driving helpers for the prepared fixture app (C03 onward): process
control, the compiled Accessibility helper, frontmost-guarded keystrokes,
fixture data-root hygiene and the synthetic scan endpoint.

Keystrokes go through System Events only after the fixture app is confirmed
frontmost, so they can never reach another application. The fixture build
keeps data in its own Application Support directory and credentials in a
dedicated fixture test keychain there; the login keychain and v1 data are
never touched.
"""

import json
import os
import shutil
import signal
import socket
import subprocess
import time

HOME = os.path.expanduser("~")
DATA = os.path.join(HOME, "Library/Application Support/io.cdot.prisma-airs-desktop.v2.fixture")
KEYCHAIN = os.path.join(DATA, "fixture-credentials.keychain-db")
ROOTS = os.path.join(DATA, "fixture-transport-roots.pem")
V1_DATA = os.path.join(HOME, "Library/Application Support/Prisma AIRS Desktop")
PROCESS = "Fixture.app/Contents/MacOS/prisma-airs-desktop-v2"
TOOLS = os.path.dirname(os.path.abspath(__file__))


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


class App:
    """One fixture bundle driven through Accessibility."""

    def __init__(self, bundle, work, log):
        self.bundle = bundle
        self.log = log
        self.ax_bin = os.path.join(work, "ax")
        built = sh(["swiftc", "-O", os.path.join(TOOLS, "ax.swift"), "-o", self.ax_bin])
        if built.returncode != 0:
            raise SystemExit(f"ax helper did not build: {built.stderr}")
        self.pid = None

    def note(self, what, **data):
        self.log.append({"t": time.strftime("%H:%M:%S"), "step": what, **data})

    # ---- process ----
    @staticmethod
    def running_pid():
        out = sh(["pgrep", "-f", PROCESS]).stdout.split()
        return int(out[0]) if out else None

    def quit(self):
        pid = self.running_pid()
        if pid:
            try:
                self.keys("q", "command down")
            except RuntimeError:
                pass
            for _ in range(40):
                time.sleep(0.25)
                if not self.running_pid():
                    break
        sh(["pkill", "-f", PROCESS])
        time.sleep(0.5)
        self.pid = None

    def kill(self):
        """Ends the process without a graceful quit (an abrupt close)."""
        pid = self.running_pid()
        if pid:
            os.kill(pid, signal.SIGKILL)
            for _ in range(20):
                time.sleep(0.25)
                if not self.running_pid():
                    break
        self.pid = None

    def launch(self, settle=6):
        self.quit()
        sh(["open", "-n", self.bundle])
        for _ in range(settle * 2):
            time.sleep(0.5)
            if self.running_pid():
                break
        self.pid = self.running_pid()
        if not self.pid:
            raise RuntimeError("the fixture app did not start")
        self.tree()  # announces the AX client so WebKit builds its tree
        tree, _ = self.wait_for(lambda t: self.node(t, "AXHeading", "") is not None, 15)
        time.sleep(0.5)
        return self.pid

    # ---- accessibility ----
    def dump_tree(self, tag):
        """Keeps the current tree beside the probe for diagnosis; returns its path and a one-line summary."""
        dump = os.path.join(os.path.dirname(self.ax_bin), f"ax-{tag}-{time.strftime('%H%M%S')}.json")
        try:
            tree = self.tree()
            with open(dump, "w") as f:
                json.dump(tree, f)
            heads = [n.get("label") for n in tree if n.get("role") == "AXHeading"][:6]
            return f"{dump} ({len(tree)} nodes; headings {heads}; window {bool(self.node(tree, 'AXWindow', ''))}; pid {self.pid} running {self.running_pid()})"
        except Exception as e:  # noqa: BLE001 - the diagnosis must not mask the failure
            return f"(tree unavailable: {e})"

    def ax(self, command, *args):
        out = sh([self.ax_bin, command, str(self.pid), *[str(a) for a in args]])
        if command == "tree":
            return json.loads(out.stdout or "[]")
        if out.returncode != 0:
            # The tree at the failure, kept beside the probe for diagnosis (a control the driver expected was not there).
            raise RuntimeError(f"ax {command} {args}: {out.stderr.strip()} [tree: {self.dump_tree('failure')}]")
        return out.stdout

    def tree(self):
        return self.ax("tree")

    @staticmethod
    def texts(tree):
        return [n.get("label") or "" for n in tree] + [str(n.get("value")) for n in tree if n.get("value") is not None]

    def has_text(self, tree, fragment):
        return any(fragment in t for t in self.texts(tree))

    @staticmethod
    def node(tree, role, prefix, nth=0):
        found = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(prefix)]
        return found[nth] if nth < len(found) else None

    @staticmethod
    def focused(tree):
        return next((n for n in tree if n.get("focused") and n.get("role") not in ("AXWindow", "AXWebArea", "AXGroup", "AXScrollArea")), None)

    def wait_for(self, predicate, timeout=12.0):
        deadline = time.time() + timeout
        while True:
            tree = self.tree()
            if predicate(tree):
                return tree, True
            if time.time() > deadline:
                return tree, False
            time.sleep(0.3)

    # ---- keyboard (frontmost-guarded) ----
    def front(self):
        script = (f'tell application "System Events"\nset p to first application process whose unix id is {self.pid}\n'
                  'set frontmost of p to true\ndelay 0.2\nreturn frontmost of p\nend tell')
        for _ in range(5):
            if sh(["osascript", "-e", script]).stdout.strip() == "true":
                return
            time.sleep(0.3)
        raise RuntimeError(f"the fixture app (pid {self.pid}) is not frontmost; refusing to send keystrokes")

    def keys(self, text, modifiers=None):
        if self.pid is None:
            self.pid = self.running_pid()
        if self.pid is None:
            raise RuntimeError("keystrokes need the fixture pid")
        self.front()
        using = f" using {{{modifiers}}}" if modifiers else ""
        escaped = text.replace("\\", "\\\\").replace('"', '\\"')
        sh(["osascript", "-e", f'tell application "System Events" to keystroke "{escaped}"{using}'])
        time.sleep(0.4)

    def keycode(self, code, modifiers=None):
        self.front()
        using = f" using {{{modifiers}}}" if modifiers else ""
        sh(["osascript", "-e", f'tell application "System Events" to key code {code}{using}'])
        time.sleep(0.35)

    def tab_walk(self, steps):
        order = []
        for _ in range(steps):
            self.keycode(48)
            f = self.focused(self.tree())
            order.append(None if f is None else {"role": f["role"], "subrole": f.get("subrole"), "label": f.get("label")})
        return order

    def menu(self, *path):
        self.front()
        chain = f'menu item "{path[-1]}"'
        for item in reversed(path[1:-1]):
            chain += f' of menu 1 of menu item "{item}"'
        chain += f' of menu 1 of menu bar item "{path[0]}" of menu bar 1'
        sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {self.pid}) to click {chain}'])
        time.sleep(0.8)

    def field(self, role, label, nth=0):
        return self.node(self.tree(), role, label, nth)

    def type_into(self, role, label, text, nth=0, secure=False):
        """Types `text` and verifies what arrived; retypes over a selection if
        keystrokes were dropped."""
        for attempt in range(3):
            self.ax("focus", role, label, nth)
            time.sleep(0.4)
            if attempt:
                self.keys("a", "command down")
                self.keycode(51)
            self.keys(text)
            time.sleep(0.3)
            value = str((self.field(role, label, nth) or {}).get("value") or "")
            if (len(value) == len(text) and set(value) <= {"•"}) if secure else value == text:
                self.note("typed", field=label, attempts=attempt + 1)
                return
        raise RuntimeError(f"could not type into {label} reliably")

    def type_file(self, role, label, path, nth=0, timeout=60, per_event=20):
        """Replaces a field's text with a file's contents typed as real key
        events (posted to the app's process only) and waits until it all
        arrived."""
        expected = open(path, encoding="utf-8").read()
        for attempt in range(3):
            self.ax("focus", role, label, nth)
            time.sleep(0.3)
            self.keys("a", "command down")
            self.keycode(51)
            self.front()
            self.ax("type-file", path, per_event)
            time.sleep(1.0)
            deadline = time.time() + timeout
            value = ""
            while time.time() < deadline:
                value = str((self.field(role, label, nth) or {}).get("value") or "")
                if len(value) >= len(expected):
                    break
                time.sleep(0.3)
            if value == expected:
                self.note("typed-file", field=label, chars=len(expected), attempts=attempt + 1)
                return True
            self.note("typed-file-mismatch", field=label, expected=len(expected), arrived=len(value), attempt=attempt + 1)
        return False

    # ---- flows ----
    def open_settings(self):
        """Opens Settings and waits until its form is in the tree (the heading
        lands before the fields under load; a second ⌘, is sent if the sheet
        did not open)."""
        ready = lambda t: self.node(t, "AXHeading", "Settings") is not None and self.node(t, "AXTextField", "TSG ID (optional)") is not None
        for _ in range(2):
            self.keys(",", "command down")
            tree, ok = self.wait_for(ready, 20)
            if ok:
                return tree
        raise RuntimeError("Settings did not open with its tenant form")

    def open_runtime(self):
        self.ax("press", "AXButton", "Runtime Security")
        tree, _ = self.wait_for(lambda t: self.node(t, "AXHeading", "Scan a prompt or response") is not None)
        return tree

    def create_custom_tenant(self, label, origin):
        self.type_into("AXTextField", "Label", label)
        self.ax("press", "AXRadioButton", "Custom HTTPS endpoint")
        self.wait_for(lambda t: self.node(t, "AXTextField", "Endpoint origin") is not None)
        self.type_into("AXTextField", "Endpoint origin", origin)
        self.keycode(36)
        return self.wait_for(lambda t: self.node(t, "AXButton", f"Delete {label}") is not None)

    def save_key(self, key):
        # The credential form re-renders when the tenant becomes active; wait for its field (the first launch after
        # a reboot, caches cold, had the field appear after the driver looked for it).
        self.wait_for(lambda t: self.node(t, "AXTextField", "AIRS API key") is not None, 12)
        self.type_into("AXTextField", "AIRS API key", key, secure=True)
        self.keycode(36)
        return self.wait_for(lambda t: self.has_text(t, "Scan key saved in the Keychain."))

    def make_active(self, label):
        """Makes `label` the active tenant from Settings' list; confirmed by Settings' active-tenant text or, since the
        v2 shell leaves Settings for Home on a switch, by the shell's tenant switcher naming it."""
        def active(t):
            # The switcher names the active tenant first ("Switch tenant: Prod · Production · TSG …"): the label is
            # matched as that first part, not as a substring (Prod is one of Production).
            return self.has_text(t, f"Active tenant: {label}") or self.has_text(t, f"{label} (active)") \
                or any((n.get("label") or "") == f"Switch tenant: {label}" or (n.get("label") or "").startswith(f"Switch tenant: {label} ·") for n in t)
        if active(self.tree()):
            return self.tree(), True
        self.ax("press", "AXButton", f"Make {label} active")
        return self.wait_for(active)


def clean_fixture_root():
    if os.path.exists(KEYCHAIN):
        sh(["security", "delete-keychain", KEYCHAIN])
    if os.path.isdir(DATA):
        shutil.rmtree(DATA)


def v1_metadata():
    entries = []
    for base, _, files in os.walk(V1_DATA):
        for name in files:
            path = os.path.join(base, name)
            try:
                st = os.lstat(path)
            except OSError:
                continue
            entries.append([os.path.relpath(path, V1_DATA), st.st_size, int(st.st_mtime)])
    return sorted(entries)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class ScanEndpoint:
    """The synthetic HTTPS scan endpoint (scan_server.py) in a scratch dir."""

    def __init__(self, directory, key):
        self.dir, self.key = directory, key
        self.port = free_port()
        self.proc = None
        made = sh(["python3", "-I", os.path.join(TOOLS, "scan_server.py"), "--dir", directory, "--port", "0", "--key", key, "--make-certs"])
        if made.returncode != 0:
            raise SystemExit(f"fixture certificates were not generated: {made.stderr}")
        self.mode("allow")

    @property
    def origin(self):
        return f"https://localhost:{self.port}"

    def install_roots(self):
        """Trust the test CA in the fixture build (read at launch)."""
        os.makedirs(DATA, exist_ok=True)
        shutil.copy(os.path.join(self.dir, "ca.pem"), ROOTS)

    def start(self):
        if self.proc:
            return
        self.proc = subprocess.Popen(
            ["python3", "-I", os.path.join(TOOLS, "scan_server.py"), "--dir", self.dir, "--port", str(self.port), "--key", self.key],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        if self.proc.stdout.readline().strip() != "ready":
            raise SystemExit("the synthetic scan endpoint did not start")

    def stop(self):
        if self.proc:
            self.proc.terminate()
            self.proc.wait(timeout=5)
            self.proc = None

    def mode(self, reply, delay_ms=0):
        with open(os.path.join(self.dir, "mode.json"), "w") as f:
            json.dump({"reply": reply, "delayMs": delay_ms}, f)

    def requests(self):
        path = os.path.join(self.dir, "requests.jsonl")
        if not os.path.exists(path):
            return []
        with open(path) as f:
            return [json.loads(line) for line in f if line.strip()]


class AcademyEndpoint:
    """The synthetic Academy + Clerk endpoint (academy_server.py)."""

    def __init__(self, directory):
        self.dir = directory
        self.port = free_port()
        self.proc = None
        made = sh(["python3", "-I", os.path.join(TOOLS, "scan_server.py"), "--dir", directory, "--port", "0", "--key", "unused", "--make-certs"])
        if made.returncode != 0:
            raise SystemExit(f"fixture certificates were not generated: {made.stderr}")
        self.mode(approval="pending")

    @property
    def origin(self):
        return f"https://localhost:{self.port}"

    def install(self):
        """Trust the test CA and point the fixture build at this endpoint
        (both read at launch)."""
        os.makedirs(DATA, exist_ok=True)
        shutil.copy(os.path.join(self.dir, "ca.pem"), ROOTS)
        with open(os.path.join(DATA, "fixture-academy-origin"), "w") as f:
            f.write(self.origin)

    def start(self):
        if self.proc:
            return
        self.proc = subprocess.Popen(
            ["python3", "-I", os.path.join(TOOLS, "academy_server.py"), "--dir", self.dir, "--port", str(self.port)],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        if self.proc.stdout.readline().strip() != "ready":
            raise SystemExit("the synthetic Academy endpoint did not start")

    def stop(self):
        if self.proc:
            self.proc.terminate()
            self.proc.wait(timeout=5)
            self.proc = None

    def mode(self, **values):
        with open(os.path.join(self.dir, "mode.json"), "w") as f:
            json.dump(values, f)

    def requests(self):
        path = os.path.join(self.dir, "requests.jsonl")
        if not os.path.exists(path):
            return []
        with open(path) as f:
            return [json.loads(line) for line in f if line.strip()]


def opened_urls():
    path = os.path.join(DATA, "fixture-opened-urls.log")
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [line.strip() for line in f if line.strip()]


def keychain_accounts(service):
    """Accounts of items in one namespace of the fixture keychain
    (attributes only; secrets are never read)."""
    if not os.path.exists(KEYCHAIN):
        return []
    out = sh(["security", "dump-keychain", KEYCHAIN]).stdout
    accounts, account, svc = [], None, None
    for line in out.splitlines() + ["keychain:"]:
        line = line.strip()
        if line.startswith("keychain:") or line.startswith("class:"):
            if svc == service and account:
                accounts.append(account)
            account, svc = None, None
        if line.startswith('"acct"<blob>='):
            account = line.split("=", 1)[1].strip('"')
        if line.startswith('"svce"<blob>='):
            svc = line.split("=", 1)[1].strip('"')
    return sorted(accounts)
