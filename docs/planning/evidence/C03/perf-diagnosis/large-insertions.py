"""Exploratory C03 memory diagnosis (see README.md). Run from the repository
root on the prepared C03 fixture bundle; uses the synthetic endpoint only."""
import json, os, sys, tempfile, time, subprocess
sys.path.insert(0, "tools/native-acceptance")
import fixture_app as fx
prepared = json.load(open("docs/planning/evidence/C03/native-prepare.json"))
work = tempfile.mkdtemp(prefix="airs-diag-")
app = fx.App(os.path.join(os.getcwd(), prepared["bundle"]), work, [])
app.quit(); fx.clean_fixture_root()
ep = fx.ScanEndpoint(os.path.join(work, "ep"), "synthetic-diag-key"); ep.install_roots(); ep.start()
def wk(): return set(int(p) for p in fx.sh(["pgrep","-f","com.apple.WebKit"]).stdout.split())
before = wk()
app.launch(); time.sleep(2)
pids = {"app": app.pid}
for p in sorted(wk()-before):
    pids[fx.sh(["ps","-o","comm=","-p",str(p)]).stdout.strip().split(".")[-1]] = p
def mem(tag):
    out = {k: int((fx.sh(["ps","-o","rss=","-p",str(v)]).stdout.strip() or 0))//1024 for k,v in pids.items()}
    print(tag, out, sum(out.values()), flush=True)
app.open_settings(); app.create_custom_tenant("T", ep.origin); app.save_key("synthetic-diag-key"); app.open_runtime()
app.type_into("AXTextField", "Profile name or ID", "P")
app.ax("focus", "AXTextArea", "Prompt")
f = os.path.join(work, "p.txt"); block="The quick brown fox jumps over the lazy dog. "; open(f,"w").write((block*3000)[:100*1024])
for chunk in ["30000"]:
    app.front(); app.keys("a","command down"); app.keycode(51)
    mem("before")
    t=time.time(); app.ax("type-file", f, chunk); 
    time.sleep(3); n = app.field("AXTextArea","Prompt"); print("chunk", chunk, "len", len(str(n.get("value") or "")), round(time.time()-t,1))
    mem("after single-event insert")
for i in range(5):
    app.front(); app.keys("a","command down"); app.keycode(51)
    app.ax("type-file", f, 30000); time.sleep(1)
    n=len(ep.requests()); app.ax("press","AXButton","Scan",0)
    while len(ep.requests())==n: time.sleep(0.1)
    time.sleep(1.5); mem(f"insert+scan {i+1}"); print(ep.requests()[-1]["promptChars"])
app.quit(); ep.stop(); fx.clean_fixture_root()
