"""C45 fixture process ownership: exact bundle, executable and PID lifetime.

No process-name signaling, argument inspection, foreign adoption or installed-app
lookup. macOS struct layout comes from the selected SDK sys/proc_info.h.
"""
from dataclasses import dataclass
from pathlib import Path
import ctypes
import hashlib
import json
import os
import plistlib
import signal
import subprocess
import sys
import time

FIXTURE_ID = 'io.cdot.airs-desktop.fixture'
EXECUTABLE = 'prisma-airs-desktop'

@dataclass(frozen=True)
class Identity:
    pid: int
    executable: str
    birth: tuple
    parent: int

class _BsdInfo(ctypes.Structure):
    _fields_ = [('header', ctypes.c_uint32 * 12), ('comm', ctypes.c_char * 16),
                ('name', ctypes.c_char * 32), ('misc', ctypes.c_uint32 * 5),
                ('nice', ctypes.c_int32), ('seconds', ctypes.c_uint64), ('micros', ctypes.c_uint64)]

class MacProcesses:
    def __init__(self):
        if sys.platform != 'darwin':
            raise RuntimeError('native process ownership requires macOS')
        self.lib = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
        self.lib.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
        self.lib.proc_pidpath.restype = ctypes.c_int
        self.lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
        self.lib.proc_pidinfo.restype = ctypes.c_int

    def snapshot(self, pid):
        path = ctypes.create_string_buffer(4096)
        info = _BsdInfo()
        if self.lib.proc_pidpath(pid, path, len(path)) <= 0:
            return None
        if self.lib.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info)) != ctypes.sizeof(info):
            return None
        if info.header[3] != pid:
            return None
        return Identity(pid, os.path.realpath(os.fsdecode(path.value)), (info.seconds, info.micros), info.header[4])

    def pids(self):
        return [int(p) for p in subprocess.check_output(['/bin/ps', '-axo', 'pid='], text=True).split()]

    def send(self, pid, sig):
        os.kill(pid, sig)

class Candidate:
    def __init__(self, bundle, processes=None, record=None):
        self.bundle = Path(bundle).resolve(strict=True)
        info = plistlib.loads((self.bundle/'Contents/Info.plist').read_bytes())
        if info.get('CFBundleIdentifier') != FIXTURE_ID or info.get('CFBundleExecutable') != EXECUTABLE:
            raise ValueError('only the chosen C45 fixture bundle is allowed')
        app_path=(self.bundle/'Contents/MacOS'/EXECUTABLE).resolve(strict=True)
        engine_path=(self.bundle/'Contents/Resources/harness/airs-harness').resolve()
        if not app_path.is_relative_to(self.bundle) or not engine_path.is_relative_to(self.bundle):
            raise ValueError('candidate executable or engine escapes its fixture bundle')
        self.app_path=str(app_path)
        self.engine_path=str(engine_path)
        self.processes = processes if processes is not None else MacProcesses()
        self.record = Path(record) if record else None
        self.app = None
        self.engines_owned = {}
        self.child = None

    def matching(self, path):
        return [s for pid in self.processes.pids() if (s := self.processes.snapshot(pid)) and s.executable == path]

    def valid(self, identity):
        if identity is None:return False
        current=self.processes.snapshot(identity.pid)
        return current is not None and (current.pid,current.executable,current.birth)==(identity.pid,identity.executable,identity.birth)

    def publish(self):
        if self.record:
            def value(s):return dict(pid=s.pid, executable=s.executable, birth=s.birth, parent=s.parent)
            data={'schema':'airs-desktop/candidate-processes/1','bundle':str(self.bundle),
                  'app':value(self.app) if self.app else None,
                  'engines':[value(s) for s in self.engines_owned.values()],
                  'meaning':'Observed exact executable/PID birth identities owned by this driver; no process-name or installed-app lookup.'}
            self.record.parent.mkdir(parents=True,exist_ok=True)
            self.record.write_text(json.dumps(data,indent=2)+'\n')

    def app_pid(self):
        matches = self.matching(self.app_path)
        if len(matches)>1:
            raise RuntimeError('multiple candidate processes; no UI or signal allowed')
        if self.valid(self.app):return self.app.pid
        if matches:
            raise RuntimeError('candidate process is not owned by this driver; inspect durable ownership')
        return None

    def adopt_child(self, pid, parent):
        identity=self.processes.snapshot(pid)
        if not identity or identity.executable!=self.app_path or identity.parent!=parent:
            raise RuntimeError('launched child identity does not match the bound fixture')
        self.app=identity;self.publish();return pid

    def launch(self):
        if self.matching(self.app_path) or self.matching(self.engine_path):
            raise RuntimeError('unowned candidate process exists; no automatic cleanup or duplicate writer')
        self.child=subprocess.Popen([self.app_path], stdin=subprocess.DEVNULL,
                                    stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        deadline=time.monotonic()+6
        while time.monotonic()<deadline:
            if self.child.poll() is not None:raise RuntimeError('fixture executable exited during launch')
            s=self.processes.snapshot(self.child.pid)
            if s and s.executable==self.app_path:return self.adopt_child(self.child.pid,os.getpid())
            time.sleep(.05)
        raise RuntimeError('fixture child identity was not observed')

    def engines(self):
        app_pid=self.app_pid()
        for identity in self.matching(self.engine_path):
            current=identity;seen=set()
            while app_pid and current and current.pid not in seen:
                if current.parent==app_pid:
                    self.engines_owned[identity.pid]=identity;break
                seen.add(current.pid);current=self.processes.snapshot(current.parent)
        self.engines_owned={pid:s for pid,s in self.engines_owned.items() if self.valid(s)}
        self.publish();return sorted(self.engines_owned)

    def signal_identity(self, identity, sig):
        if not self.valid(identity):return False
        self.processes.send(identity.pid,sig);return True

    def signal_app(self, sig=signal.SIGTERM):
        self.engines()  # establish owned descendants before their parent exits
        return self.signal_identity(self.app,sig)

    def signal_engine(self,pid,sig=signal.SIGKILL):
        self.engines();return self.signal_identity(self.engines_owned.get(pid),sig)

    def stop_engines(self):
        for pid in self.engines():self.signal_engine(pid)

_bound = None

def bind(bundle):
    global _bound
    path=Path(bundle).resolve(strict=True)
    if _bound is not None and _bound.bundle != path:
        raise RuntimeError('one driver cannot switch fixture candidates')
    if _bound is None:
        prepare=os.environ.get('AIRS_NATIVE_PREPARE')
        if prepare:
            p=json.loads(Path(prepare).read_text());expected=(Path.cwd()/p['bundle']).resolve(strict=True)
            if path!=expected:raise RuntimeError('bundle differs from declared native preparation')
            executable=path/'Contents/MacOS'/EXECUTABLE
            if hashlib.sha256(executable.read_bytes()).hexdigest()!=p['executable_sha256']:
                raise RuntimeError('prepared executable changed')
        _bound=Candidate(path,record=os.environ.get('AIRS_NATIVE_PROCESS_RECORD'))
    return _bound

def current():
    if _bound is not None:return _bound
    prepare=os.environ.get('AIRS_NATIVE_PREPARE')
    if prepare:return bind(Path.cwd()/json.loads(Path(prepare).read_text())['bundle'])
    return None

def app_pid():
    candidate=current();return candidate.app_pid() if candidate else None

def engine_pids():
    candidate=current();return candidate.engines() if candidate else []

def stop_app(sig=signal.SIGTERM):
    candidate=current()
    if candidate:candidate.signal_app(sig)

def stop_engines():
    candidate=current()
    if candidate:candidate.stop_engines()

def signal_engine(pid,sig=signal.SIGKILL):
    candidate=current();return candidate.signal_engine(pid,sig) if candidate else False
