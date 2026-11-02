from pathlib import Path
import argparse,hashlib,json,os,plistlib,stat,datetime

def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
 return h.hexdigest()

def inventory(root):
 entries=[]
 def walk(directory):
  for p in sorted(directory.iterdir(),key=lambda p:p.name):
   m=p.lstat()
   if stat.S_ISDIR(m.st_mode):walk(p)
   elif stat.S_ISLNK(m.st_mode):entries.append({'path':p.relative_to(root).as_posix(),'mode':'symlink','sha256':hashlib.sha256(os.fsencode(os.readlink(p))).hexdigest()})
   elif stat.S_ISREG(m.st_mode):entries.append({'path':p.relative_to(root).as_posix(),'mode':'100755' if m.st_mode & 0o111 else '100644','sha256':digest(p)})
   else:raise RuntimeError('unsupported special entry '+str(p))
 walk(root);entries.sort(key=lambda e:e['path'])
 wire=''.join(e['path']+'\0'+e['mode']+'\0'+e['sha256']+'\n' for e in entries).encode()
 return hashlib.sha256(wire).hexdigest(),entries

parser=argparse.ArgumentParser()
parser.add_argument('--repo',required=True)
parser.add_argument('--output',required=True)
parser.add_argument('--label',required=True)
a=parser.parse_args();r=Path(a.repo);out=Path(a.output)
prepfile=r/'docs/planning/evidence/P09/native-prepare.json'
prep=json.loads(prepfile.read_text());bundle=r/prep['bundle']
first,entries=inventory(bundle)
with (bundle/'Contents/Info.plist').open('rb') as f:info=plistlib.load(f)
identifier=info.get('CFBundleIdentifier')
exe=digest(bundle/'Contents/MacOS/prisma-airs-desktop-v2')
second,_=inventory(bundle)
canonical_sources={rel:digest(r/rel) for rel in ['crates/xtask/src/fingerprint.rs','crates/xtask/src/native.rs']}
errors=[]
if first!=second:errors.append('bundle changed during inspection')
if first!=prep['bundle_sha256']:errors.append('bundle differs from preparation')
if exe!=prep['executable_sha256']:errors.append('executable differs from preparation')
if identifier!=prep['identifier'] or identifier!='io.cdot.prisma-airs-desktop.v2.fixture':errors.append('fixture identifier differs')
record={'schema':'airs/root-readonly-bundle-inspection/1','label':a.label,'recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'limitations':['Read-only instantaneous artifact inspection; no native check, review score or evidence reuse is asserted. Repeat at final acceptance.','Python implementation mirrors fingerprint.rs tree_hash/file_entry/inventory_hash, using relative path, executable bit or symlink target, sorted entries and NUL/newline delimiters.'],'preparation_sha256':digest(prepfile),'inspection_script_sha256':digest(Path(__file__)),'canonical_source_sha256':canonical_sources,'bundle':prep['bundle'],'expected_bundle_sha256':prep['bundle_sha256'],'actual_bundle_sha256':first,'bundle_sha256_after':second,'expected_executable_sha256':prep['executable_sha256'],'actual_executable_sha256':exe,'expected_identifier':prep['identifier'],'actual_identifier':identifier,'file_entries':entries,'errors':errors,'matched':not errors}
with out.open('x') as f:json.dump(record,f,indent=2);f.write('\n')
print(json.dumps({k:record[k] for k in ['label','matched','actual_bundle_sha256','actual_identifier','errors']}))
raise SystemExit(bool(errors))
