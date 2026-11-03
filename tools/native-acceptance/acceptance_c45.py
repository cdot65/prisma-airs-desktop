"""Current identity/presentation native assertions. Synthetic fixture only.

Production-interface setup/reset and disposable-file Keychain isolation are
separately required by the qualified-coverage consumer. This driver does not
read actual prior installations or infer those regressions from UI absence.
"""
from pathlib import Path
import json,os,platform,plistlib,shutil,sys,tempfile,time
sys.path.insert(0,str(Path(__file__).resolve().parent))
import fixture_app as fx
import acceptance_c29 as c29
import v2_setup as setup

ROOT=Path.cwd().resolve()
BASE=ROOT/'docs/planning/evidence/C45/baseline-03'
LOG=[]


def main():
    smoke='--smoke' in sys.argv
    out=BASE/('startup-smoke-01' if smoke else 'identity')
    assert not out.exists(),'preserve prior actual driver attempt'
    out.mkdir(parents=True)
    prepared=json.loads((BASE/'native-prepare.json').read_text())
    bundle=ROOT/prepared['bundle']
    info=plistlib.loads((bundle/'Contents/Info.plist').read_bytes())
    production=json.loads((ROOT/'apps/desktop/src-tauri/tauri.conf.json').read_text())
    assert info['CFBundleIdentifier']=='io.cdot.airs-desktop.fixture' and info['CFBundleExecutable']=='prisma-airs-desktop'
    assert info['CFBundleShortVersionString']=='1.0.0' and production['productName']=='Prisma AIRS Desktop' and production['version']=='1.0.0' and production['identifier']=='io.cdot.airs-desktop'
    work=Path(tempfile.mkdtemp(prefix='airs-c45-identity-'))
    before=fx.v1_metadata();app=fx.App(str(bundle),str(work),LOG)
    observations={'plist':info,'production_config':production,'controlled_prior_before':before,'scope':'Only generated prior-installation canaries; actual old roots and login Keychain are never read.'}
    def save(name,tree):
        path=out/(name+'.json');path.write_text(json.dumps(tree,indent=1)+'\n');return str(path.relative_to(ROOT))
    try:
        app.quit();fx.clean_fixture_root();app.launch()
        tree,first=app.wait_for(lambda t:app.node(t,'AXHeading',setup.WELCOME) is not None,20)
        assert first,'fresh candidate must show wizard'
        observations['wizard']=save('fresh-wizard',tree)
        assert not any('SYNTHETIC-PRIOR-' in text for text in app.texts(tree))
        # Inspect the native About panel without completing any regional/live setup.
        app.front()
        script=f'''tell application "System Events" to tell (first application process whose unix id is {app.pid})
set appMenu to menu bar item 2 of menu bar 1
set appMenuName to name of appMenu
if appMenuName does not start with "Prisma AIRS Desktop" then error "Unexpected application menu: " & appMenuName
set aboutItem to first menu item of menu 1 of appMenu whose name starts with "About "
click aboutItem
return appMenuName
end tell'''
        result=fx.sh(['osascript','-e',script]);assert result.returncode==0,result.stderr
        observations['actual_native_application_menu']=result.stdout.strip()
        tree,about=app.wait_for(lambda t:app.has_text(t,'1.0.0'),10)
        observations['about']=save('about',tree)
        assert about and app.has_text(tree,'Prisma AIRS Desktop'),'native About must expose product/version'
        app.keys('w','command down')
        app.quit();app.launch()
        tree,stillfresh=app.wait_for(lambda t:app.node(t,'AXHeading',setup.WELCOME) is not None,20)
        assert stillfresh,'relaunch before setup must remain fresh'
        observations['fresh_relaunch']=save('fresh-relaunch',tree)
        # Explicitly recorded controlled fixtures are checked again after teardown.
        observations['owned_processes']=json.loads(Path(os.environ['AIRS_NATIVE_PROCESS_RECORD']).read_text())
        app.quit();fx.clean_fixture_root()
        observations['controlled_prior_after']=fx.v1_metadata()
        assert observations['controlled_prior_after']==before,'controlled prior canaries changed'
        observations['log']=LOG
        obs=out/'observations.json';obs.write_text(json.dumps(observations,indent=2)+'\n')
        if smoke:
            (out/'smoke.json').write_text(json.dumps({'status':'pass','executable_sha256':prepared['executable_sha256'],'checks':'real direct launch, fresh wizard, native About1.0.0, owned PID controls, fresh relaunch, controlled canary preservation; not cumulative C45 acceptance'},indent=2)+'\n')
            return 0
        # Mapped cumulative assertions and production-interface checks are consumed
        # by qualification; do not invent a full native/UI proof from this smoke.
        checks=[{'id':'C45-N01','result':'pass','observation':'Exact renamed fixture/config1.0.0, native About product/version and fresh launch/relaunch are observed. Home-first catalogue, Settings/Sessions focus and headings are required by the same candidate C44/C11 and shell-port baseline, not inferred here.','evidence':[str(obs.relative_to(ROOT)),observations['wizard'],observations['about']]},
                {'id':'C45-N02','result':'pass','observation':'Fresh wizard and fresh relaunch expose no controlled prior canary, and controlled alpha/v2 canaries remain byte-identical across fixture cleanup/relaunch. Actual FileStore setup/restart/reset and four service isolation are separate required production-interface checks in qualification.','evidence':[str(obs.relative_to(ROOT)),observations['fresh_relaunch']]},
                {'id':'C45-N03','result':'pass','observation':'The current same-candidate mapped native suite and production-interface/frontend regressions are separately required by qualification for ordinary collections, active helpers, removed intents and all current native assertions. This row explicitly binds their required aggregate rather than claiming they ran within this driver.','evidence':[str((BASE/'native-acceptance.json').relative_to(ROOT)),str(obs.relative_to(ROOT))]}]
        aggregate=json.loads((BASE/'native-acceptance.json').read_text());assert aggregate['app']['executable_sha256']==prepared['executable_sha256'] and all(c['result']=='pass' for c in aggregate['checks'])
        inventory={row['id']:row['check'] for row in json.loads((ROOT/'docs/architecture/native-checks.json').read_text())['chunks']['C45']}
        for check in checks:check['description']=inventory[check['id']]
        (out/'native-acceptance.json').write_text(json.dumps({'schema':'airs-v2/native-acceptance/1','subject':'C45','method':'cua','performed_by':os.environ['AIRS_NATIVE_ORCHESTRATOR'],'host':platform.platform(),'recorded_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'app':{'bundle':prepared['bundle'],'identifier':prepared['identifier'],'executable_sha256':prepared['executable_sha256'],'fixture':True},'checks':checks,'limitations':['No actual prior user data or login Keychain accessed; no product calls. Non-adoption/reset/services are separately qualified production-interface regressions. Spoken VoiceOver/signing/installation not claimed.']},indent=2)+'\n')
        return 0
    finally:
        fx.stop_app();fx.stop_engines();shutil.rmtree(work,ignore_errors=True)


if __name__=='__main__':raise SystemExit(main())
