from pathlib import Path
import datetime,hashlib,json,subprocess,sys

root=Path.cwd().resolve();subject,attempt=sys.argv[1:3];out=root/f'docs/planning/reviews/{subject}/{attempt}'
def read(p):return json.loads(Path(p).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def validate(value,schema,where='$'):
    kind=schema.get('type');types={'object':dict,'array':list,'string':str,'integer':int}
    if kind:assert type(value) is types[kind],where
    if kind=='object':
        assert set(schema.get('required',[]))<=set(value),where
        properties=schema.get('properties',{})
        if schema.get('additionalProperties') is False:assert set(value)<=set(properties),where
        for k,v in value.items():
            if k in properties:validate(v,properties[k],where+'.'+k)
    if kind=='array':
        for n,v in enumerate(value):validate(v,schema['items'],where+f'[{n}]')
    if kind=='integer':assert schema.get('minimum',value)<=value<=schema.get('maximum',value),where
    if 'enum' in schema:assert value in schema['enum'],where
r=read(out/'request.json');v=read(out/'verdict-returned.json');validate(v,read(root/'docs/architecture/review-verdict.schema.json'))
assert r['subject']==v['subject']==subject and r['attempt_id']==v['attempt_id']==attempt
assert r['request_id']==v['request_id'] and r['fingerprints']==v['fingerprints']
i=read(out/'source-inventory.json');assert set(i['files'])==set(r['scope']['files'])
for p,h in i['files'].items():assert sha(root/p)==h,'source drift '+p
source=hashlib.sha256(''.join(f'{p} {h}\n' for p,h in sorted(i['files'].items())).encode()).hexdigest()
assert source==i['source_fingerprint']==r['fingerprints']['source']
command=r['commands'][0];run=read(root/command['record']);assert sha(root/command['record'])==r['fingerprints']['evidence']
assert len(run['runs'])==18
for name,x in run['runs'].items():assert x['exit']==0 and sha(root/x['log'])==x['log_sha256'],name
assert run['inputs']['before_digest']==run['inputs']['after_digest']==r['fingerprints']['dependencies']
assert run['build_flags']['digest']==r['fingerprints']['build_flags']
n=r['native_acceptance'];assert sha(root/n['prepare'])==n['prepare_sha256']
current=read(root/n['prepare']);assert sha(root/current['bundle']/'Contents/MacOS/prisma-airs-desktop-v2')==current['executable_sha256']
for native in n['mapped_records']:
    p=root/native['record'];assert sha(p)==native['sha256'];record=read(p)
    assert sha(root/native['prepare'])==native['prepare_sha256']
    prep=read(root/native['prepare']);assert record['app']['executable_sha256']==prep['executable_sha256']==native['executable_sha256']
    assert record['app']['executable_sha256']==current['executable_sha256'],'new reviews require the current integrated native bundle'
    checks={c['id']:c for c in record['checks']}
    for t in native['required_targets']:assert checks[t]['result']=='pass' and all((root/e).exists() for e in checks[t]['evidence'])
    if native['subject']=='C43':
        sys.path.insert(0,str(root/'tools/native-acceptance'))
        import wizard_coverage
        assert not wizard_coverage.problems(read(p.parent/'native/observations.json')['observations'],root)
assert sha(root/n['live_row'])==n['live_row_sha256'];live=read(root/n['live_row']);assert live['status']=='pass'
raw=read(root/live['native_receipt']);assert sha(root/live['native_receipt'])==live['native_receipt_sha256']
assert raw['status']=='pass' and raw['local_credentials_removed']
build=read((root/live['native_receipt']).with_name('build.json'))
# The raw build record keeps its historical absolute path. The archived
# executable provides the same exact byte check after cloning on Worf.
qualification_exe=Path(build['bundle'])/'Contents/MacOS/prisma-airs-desktop-v2'
if not qualification_exe.is_file():
    qualification_exe=root/'docs/planning/evidence/C29/live/L19/build-02/l19-qualification-executable.bin'
assert sha(qualification_exe)==raw['executable_sha256']
scratch=root/'.evidence-private/p09-review'
scratch.mkdir(parents=True,exist_ok=True)
post=scratch/f'post-{subject}-{attempt}-inputs.sha256'
digest=subprocess.check_output(['/bin/bash',str(root/'docs/planning/evidence/P09/runs/hash-inputs.sh'),str(root),str(post)],text=True).strip()
assert digest==r['fingerprints']['dependencies']
result={'subject':subject,'attempt':attempt,'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'raw_verdict_sha256':sha(out/'verdict-returned.json'),'request_sha256':sha(out/'request.json'),
        'source_files_verified':len(i['files']),'check_runs_verified':18,'compiled_inputs_unchanged':True,
        'live_row_pass_verified':True,'scores':v['scores'],
        'd034_eligible':min(v['scores'].values())>=8 and not v['blockers'],
        'full_gate_pass':min(v['scores'].values())>=9 and v['recommendation']=='pass' and not any(v[k] for k in ['required_fixes','blockers','evidence_gaps'])}
(out/'post-review-audit.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
