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
    return value
