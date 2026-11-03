"""Candidate/source bindings shared by C45 qualification and actual reviews."""
from pathlib import Path
import ast
import os
import stat
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'p09'))
from phase_packet import source_inventory, file_identity, digest
from qualification import read, sha, require


def bundle_hash(bundle):
    entries=[]
    for base, directories, files in os.walk(bundle,followlinks=False):
        symlinks=[name for name in directories if (Path(base)/name).is_symlink()]
        directories[:]=[name for name in directories if name not in symlinks]
        for name in files+symlinks:
            path=Path(base)/name;relative=str(path.relative_to(bundle))
            if path.is_symlink():mode='symlink';value=__import__('hashlib').sha256(os.readlink(path).encode()).hexdigest()
            else:mode='100755' if path.stat().st_mode & 0o111 else '100644';value=sha(path)
            entries.append((relative,mode,value))
    text=''.join(f'{name}\0{mode}\0{value}\n' for name,mode,value in sorted(entries))
    return __import__('hashlib').sha256(text.encode()).hexdigest()


def helper_closure(root, driver):
    folder=root/'tools/native-acceptance';pending=[driver];seen=set()
    while pending:
        name=pending.pop()
        if name in seen or not (folder/(name+'.py')).is_file():continue
        seen.add(name);tree=ast.parse((folder/(name+'.py')).read_text())
        for node in ast.walk(tree):
            if isinstance(node,ast.Import):pending.extend(alias.name.split('.')[0] for alias in node.names)
            elif isinstance(node,ast.ImportFrom) and node.module:pending.append(node.module.split('.')[0])
    paths=['tools/native-acceptance/'+name+'.py' for name in seen]
    # Native compiled helpers/servers are used through literal paths, not imports.
    paths += [str(p.relative_to(root)) for p in folder.iterdir() if p.suffix in ['.swift','.js']]
    paths += ['tools/p10/native.py','tools/p10/candidate.py','docs/planning/evidence/P09/native-controller/native_requirements.py']
    return {name:sha(root/name) for name in sorted(set(paths))}


def verify_source(root, seal):
    expected=read(root/seal)['files']
    require(source_inventory(root)==expected,'sealed source membership/content changed')
    return digest(expected)


def verify_candidate(root, value):
    prepared=read(root/value['prepare']['path'])
    require(sha(root/value['prepare']['path'])==value['prepare']['sha256'],'preparation drift')
    bundle=root/prepared['bundle']
    require(sha(bundle/'Contents/MacOS/prisma-airs-desktop')==value['executable_sha256']==prepared['executable_sha256'],'candidate executable drift')
    require(bundle_hash(bundle)==value['bundle_sha256']==prepared['bundle_sha256'],'complete candidate bundle drift')
    for name,value_hash in value['helper_closure'].items():require(sha(root/name)==value_hash,'candidate helper drift: '+name)
    if value.get('build_origin'):
        import platform
        origin=value['build_origin'];path=root/origin['record']
        require(sha(path)==origin['sha256'],'build origin proof drift')
        audit=verify_build_reuse(root,read(path),value['runner_inputs'],platform.platform(),platform.node())
        require(audit==origin['audit'],'build reuse audit drift')
    return value

# Explicit helper/provenance-only transition after the baseline02 startup smoke.
# No application/config/dependency path is permitted. Old raw build and failed
# smoke remain immutable; this is not a new execution of that build.
BUILD_REUSE_PATHS = {
    'tools/native-acceptance/acceptance_c45.py',
    'tools/p10/candidate.py', 'tools/p10/test_build_reuse.py',
    'tools/p10/supervise.py', 'tools/p10/seal.py',
    'tools/p10/finalize.py', 'tools/p10/native.py',
    'apps/desktop/src-tauri/tests/permission_manifest.rs',
    'tools/p10/continuation.py', 'tools/p10/test_continuation.py',
    'tools/native-acceptance/acceptance_c32_current.py',
    'tools/native-acceptance/test_c32_project_path.py',
}


def verify_build_reuse(root, record, current_identity, platform_name, hostname):
    from qualification import bound
    from stages import verify_completed
    require(record['schema']=='airs-desktop/c45-helper-only-build-reuse/1','build reuse schema')
    old=read(bound(root,record['original_candidate']))
    before=read(bound(root,record['original_source']))['files']
    after=read(bound(root,record['current_source']))['files']
    changed=sorted(name for name in set(before)|set(after) if before.get(name)!=after.get(name))
    require(changed==record['declared_changed_paths'] and set(changed)<=BUILD_REUSE_PATHS,'undeclared or application-affecting build-reuse delta')
    require(source_inventory(root)==after,'current build-reuse source changed')
    require(digest(before)==old['source_inventory_sha256'],'original source seal/candidate mismatch')
    require(current_identity['dependencies']==old['runner_inputs']['dependencies'] and current_identity['build_flags']==old['runner_inputs']['build_flags'],'build dependency/toolchain/flags changed')
    require(old['conditions']['platform']==platform_name and old['conditions']['hostname']==hostname,'build execution conditions changed')
    state_path=bound(root,record['original_build_stage']);state=read(state_path)
    verify_completed(root,state_path.parent,state,state['argv'],digest(before))
    prepared=read(bound(root,old['prepare']));bundle=root/prepared['bundle']
    require(sha(bundle/'Contents/MacOS/prisma-airs-desktop')==old['executable_sha256']==prepared['executable_sha256'],'retained executable changed')
    require(bundle_hash(bundle)==old['bundle_sha256']==prepared['bundle_sha256'],'retained complete bundle changed')
    return {'original_candidate_sha256':record['original_candidate']['sha256'],'original_build_stage_sha256':record['original_build_stage']['sha256'],'declared_helper_only_delta':changed,'executable_sha256':old['executable_sha256'],'bundle_sha256':old['bundle_sha256'],'new_build_executed':False,'meaning':'Exact unchanged application retained from actual baseline02 build; changed C45 selector and qualification provenance execute current affected verification.'}
