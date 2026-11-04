"""Prepared only. Root must supply actual build08 hashes and own GUI writer."""
from pathlib import Path
import argparse,datetime,fcntl,hashlib,json,os,plistlib,re,stat,subprocess,sys,time

CORE=Path("/Users/cdot/development/redtail/p09-queue/refinement-20261104/core")
FOLDER=Path(__file__).resolve().parent
DB=Path("/Users/admin/Library/Application Support/io.cdot.airs-desktop.fixture/fixture-credentials.keychain-db")
APP=Path("/Users/cdot/development/redtail/p09-queue/refinement-20261104/tooling/targets/core/aarch64-apple-darwin/release/bundle/macos/Prisma AIRS Desktop Fixture.app")
SERVICE="io.cdot.airs-desktop.fixture.scan"
ACCOUNT="cr-bf6eae1f4e49ee61"
PUBLIC_PASSWORD="prisma-airs-desktop-v2-fixture-keychain"
OLD=["3950c3d4b7e787d94930e3aac9bcd191cf70dd02","a579105e69b36d05fc8e5f66760688e60b8bb4c2","779bee61614329414dea277c78d41e4937dae8c2"]
TOOL=FOLDER.parent/"append-fixture-app-acl-v05"
SOURCE=TOOL.with_suffix(".c")
SOURCE_SHA="93193d76ed5e3c3f4b8b050b04d9847d74de5cea272da7b2c26731c6acc9a778"
TOOL_SHA="cc1fae6065b2511ca6546bec5897ecbcbc5ab8b5fcfc710d6637f8fb5320b12a"
PREPARE=CORE/"docs/planning/evidence/P11/native-build-08-vm/native-prepare.json"
LABEL="io.cdot.airs-fixture-acl-20261104-06"

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def stamp():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def require(ok,message):
    if not ok:raise RuntimeError(message)
def write_new(path,value):
    with path.open("x") as f:f.write(json.dumps(value,indent=2)+"\n")
def target_metadata(text):
    header=r'(?m)^(?:keychain:\s*)?\s*"'+re.escape(str(DB))+r'"\s*\nversion:'
    starts=[m.start() for m in re.finditer(header,text)]
    require(starts,"explicit fixture DB metadata header missing")
    rows=[text[start:(starts[i+1] if i+1<len(starts) else len(text))] for i,start in enumerate(starts)]
    rows=[row for row in rows if 'class: "genp"' in row and
          '"acct"<blob>="'+ACCOUNT+'"' in row and '"svce"<blob>="'+SERVICE+'"' in row]
    require(len(rows)==1,"exact fixture service/account metadata must be unique")
    require('password:' not in rows[0] and '\ndata:' not in rows[0],"unexpected value output")
    return rows[0]
def dump_target():
    result=subprocess.run(["/usr/bin/security","dump-keychain","-a",str(DB)],capture_output=True,text=True,check=True)
    return target_metadata(result.stdout)
def requirements(text):
    return re.findall(r'(?m)^\s+requirement: cdhash H"([0-9a-f]{40})"\s*$',text)
def exact_after(before,after,new):
    lines=after.splitlines(keepends=True)
    matches=[i for i,line in enumerate(lines) if re.fullmatch(r'\s+3: '+re.escape(str(APP))+r' \(status -?\d+\)\n?',line)]
    require(len(matches)==1,"one exact new trusted-app index3 required")
    i=matches[0]
    require(i+1<len(lines) and lines[i+1].strip()=='requirement: cdhash H"'+new+'"',
            "new index3 requirement differs")
    del lines[i:i+2]
    normalized="".join(lines)
    require(normalized.count("applications (4):")==1,"one changed applications count required")
    normalized=normalized.replace("applications (4):","applications (3):",1)
    require(normalized==before,"metadata differs beyond exact appended app/count")

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--prepare-sha256",required=True)
    parser.add_argument("--executable-sha256",required=True)
    parser.add_argument("--bundle-sha256",required=True)
    parser.add_argument("--new-cdhash",required=True)
    parser.add_argument("--execute-reviewed",action="store_true")
    args=parser.parse_args()
    require(args.execute_reviewed,"root-reviewed explicit execution required")
    for value in (args.prepare_sha256,args.executable_sha256,args.bundle_sha256):
        require(re.fullmatch("[0-9a-f]{64}",value) is not None,"actual lowercase SHA256 required")
    require(re.fullmatch("[0-9a-f]{40}",args.new_cdhash) is not None and args.new_cdhash not in OLD,"fresh exact CDHash required")
    require(os.getuid()==501 and os.geteuid()==501,"VM admin501 only")
    require(sha(SOURCE)==SOURCE_SHA and sha(TOOL)==TOOL_SHA,"reviewed tool/source changed")
    require(sha(PREPARE)==args.prepare_sha256,"actual build08 prepare hash differs")
    sys.path.insert(0,str(CORE/"tools/native-acceptance"))
    from acceptance_p11 import verify_prepare
    prepared,bundle=verify_prepare(PREPARE)
    require(bundle==APP,"unexpected fixture path")
    require(prepared["executable_sha256"]==args.executable_sha256 and prepared["bundle_sha256"]==args.bundle_sha256,
            "actual build08 executable/full-bundle guard differs")
    lock=open("/private/tmp/prisma-airs-desktop-native-writer.lock","a+")
    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    app=str(APP/"Contents/MacOS/prisma-airs-desktop")
    require(not [line for line in subprocess.check_output(["/bin/ps","-axo","pid=,comm="],text=True).splitlines() if app in line],
            "owned fixture application must be stopped")
    run=FOLDER/"execution"
    run.mkdir(mode=0o700)
    db_stat=DB.lstat()
    require(stat.S_ISREG(db_stat.st_mode) and db_stat.st_uid==os.getuid(),"explicit fixture DB must be an owned regular file")
    # This public fixture password unlocks only the explicit synthetic DB.
    subprocess.run(["/usr/bin/security","unlock-keychain","-p",PUBLIC_PASSWORD,str(DB)],check=True,capture_output=True)
    before=dump_target()
    require(requirements(before)==OLD and before.count("applications (3):")==1,"expected exact old3950/build05a579/build06779bee app requirements")
    with (run/"before.txt").open("x") as f:f.write(before)
    target="gui/501/"+LABEL
    plist=run/"job.plist"
    config={"Label":LABEL,"ProgramArguments":[str(TOOL),"--apply","--cdhash",args.new_cdhash],"WorkingDirectory":str(CORE),
            "RunAtLoad":True,"KeepAlive":False,"LimitLoadToSessionType":"Aqua","ProcessType":"Interactive",
            "StandardOutPath":str(run/"apply.stdout"),"StandardErrorPath":str(run/"apply.stderr")}
    with plist.open("xb") as f:f.write(plistlib.dumps(config))
    start=stamp();pids=[];exit_code=None;bootstrapped=False;cleanup=None;error=None;semantic=False
    try:
        subprocess.run(["/bin/launchctl","bootstrap","gui/501",str(plist)],check=True)
        bootstrapped=True
        print("Exact synthetic fixture ACL GUI job started",LABEL,flush=True)
        deadline=time.monotonic()+900
        while exit_code is None:
            require(time.monotonic()<deadline,"GUI prompt still pending after15minutes; receipt retained")
            state=subprocess.run(["/bin/launchctl","print",target],capture_output=True,text=True,check=True).stdout
            match=re.search(r"^\s*pid = (\d+)\s*$",state,re.M)
            if match:
                pid=int(match.group(1))
                if pid not in pids:pids.append(pid);print("GUI helper pid",pid,flush=True)
            else:
                match=re.search(r"^\s*last exit code = (\d+)\s*$",state,re.M)
                if match:exit_code=int(match.group(1))
            time.sleep(.5)
        after=dump_target()
        with (run/"after.txt").open("x") as f:f.write(after)
        require(exit_code==0,"ACL helper failed; preserve actual status/raw")
        require(requirements(after)==[*OLD,args.new_cdhash],"trusted requirements differ")
        exact_after(before,after,args.new_cdhash)
        verify_prepare(PREPARE)
        semantic=True
    except BaseException as exc:
        error=type(exc).__name__+": "+str(exc)
    finally:
        if bootstrapped:
            cleanup=subprocess.run(["/bin/launchctl","bootout",target],capture_output=True,text=True).returncode
        record={"schema":"airs-desktop/p11-fixture-code-access/2","started_utc":start,"finished_utc":stamp(),
                "host":"worf-vm","gui_label":LABEL,"observed_pids":pids,"argv":config["ProgramArguments"],
                "exit_code":exit_code,"error":error,"semantic_metadata_verified":semantic,
                "source_sha256":sha(SOURCE),"binary_sha256":sha(TOOL),"prepare_sha256":sha(PREPARE),
                "executable_sha256":args.executable_sha256,"bundle_sha256":args.bundle_sha256,
                "preserved_cdhashes":OLD,"added_cdhash":args.new_cdhash,"item_values_read":False,"item_values_modified":False,
                "owner_keychains_accessed":False,"temporary_gui_job_removed":cleanup==0,
                "artifacts":{p.name:sha(p) for p in run.iterdir() if p.is_file()}}
        write_new(run/"receipt.json",record)
        print(json.dumps(record),flush=True)
    if not semantic:raise SystemExit(1)
if __name__=="__main__":main()
