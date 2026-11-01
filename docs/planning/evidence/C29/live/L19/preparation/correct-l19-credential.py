from pathlib import Path
import datetime,hmac,json,subprocess,tomllib

base=Path('/Users/cdot/development/redtail/p09-queue/codex-c44-preflight')
source=Path('/Users/cdot/.config/airs-cli/tenants/vulture.toml')
service='io.cdot.prisma-airs-desktop.v2.live'
account='mgmt-vulture-client-secret'

def fail(message):
    raise SystemExit(message)

try:
    config=tomllib.loads(source.read_text())
    active=json.loads(Path('/Users/cdot/airs-v2-live.json').read_text())
    expected=active['families']['L19']['nonsecret']
    if config['mgmtClientId']!=expected['client_id'] or str(config['mgmtTsgId'])!=expected['tsg_id']:
        fail('public management metadata changed; no credential update performed')
    supplied=config['mgmtClientSecret'].encode('utf-8')
    if not supplied or len(supplied)>16*1024:
        fail('CLI management secret is unusable; no update performed')
    argv=['security','find-generic-password','-w','-s',service,'-a',account]
    old=subprocess.run(argv,capture_output=True)
    if old.returncode:
        fail('existing approved reference unavailable (OS exit '+str(old.returncode)+'); no update performed')
    previous=old.stdout.removesuffix(b'\n')
    old.stdout=b''
    different=not hmac.compare_digest(previous,supplied)
    updated=False
    if different:
        changed=subprocess.run([str(base/'update-vulture-keychain')],input=supplied,capture_output=True)
        if changed.returncode:
            fail('approved local Keychain correction failed; no live probe permitted')
        verified=subprocess.run(argv,capture_output=True)
        if verified.returncode or not hmac.compare_digest(verified.stdout.removesuffix(b'\n'),supplied):
            fail('local correction could not be verified; no live probe permitted')
        verified.stdout=b''
        updated=True
    record={'schema':'airs-v2/l19-local-credential-correction/1',
            'recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'owner_approval':'Compare, refresh if different, allow 1 more probe',
            'reference':{'service':service,'account':account},'source':str(source),
            'source_field':'mgmtClientSecret','public_client_id':expected['client_id'],
            'tsg_id':expected['tsg_id'],'compared':True,'different':different,
            'updated':updated,'verified':updated,'secret_values_or_hashes_retained':False,
            'original_read_reservations_retained':2,
            'additional_probe_permitted_after_correction':1 if updated else 0}
    p=base/'L19-credential-correction.json'
    p.write_text(json.dumps(record,indent=2)+'\n')
    p.chmod(0o600)
    supplied=b'';previous=b'';config.clear()
    print(json.dumps({'compared':True,'different':different,'updated_and_verified':updated,
                      'additional_probe_after_correction':1 if updated else 0}))
except SystemExit:
    raise
except Exception as error:
    raise SystemExit('private comparison failed ('+type(error).__name__+'); no values logged') from None
