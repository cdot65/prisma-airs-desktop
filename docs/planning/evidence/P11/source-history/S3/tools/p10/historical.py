"""Explicit immutable source/application context for historical qualification.

This validates what actually ran, not reuse on a current builder. Current reuse
continues to check its own source, helpers, dependencies and machine conditions.
The caller must bind this context by hash in the qualified/release proof.
"""
from pathlib import Path
from qualification import read,sha,require,bound
from candidate import file_identity,digest,bundle_hash


class Historical:
    def __init__(self,root,reference):
        self.root=root;self.reference=reference;self.record=read(bound(root,reference));self.checked={}
        require(self.record['schema']=='airs-desktop/historical-qualification-context/1','historical context schema')

    def archive_path(self,name):
        path=self.root/name
        require(path.resolve().is_relative_to((self.root/'docs/planning/evidence/C45').resolve()),'historical archive escapes C45 evidence')
        return path

    def source(self,fingerprint):
        entry=self.record['sources'].get(fingerprint);require(entry,'historical source archive missing')
        if fingerprint not in self.checked:
            seal=read(bound(self.root,entry['seal']));files=seal['files'];require(digest(files)==fingerprint,'historical source inventory binding')
            folder=self.archive_path(entry['path'])
            actual={str(p.relative_to(folder)):file_identity(p) for p in folder.rglob('*') if p.is_file() or p.is_symlink()}
            require(actual==files,'historical source archive membership/content drift')
            self.checked[fingerprint]=folder
        return self.checked[fingerprint]

    def application(self,candidate):
        entry=self.record['applications'].get(candidate['bundle_sha256']);require(entry,'historical application archive missing')
        folder=self.archive_path(entry['path'])
        require(bundle_hash(folder)==candidate['bundle_sha256'],'historical complete bundle drift')
        require(sha(folder/'Contents/MacOS/prisma-airs-desktop')==entry['executable_sha256']==candidate['executable_sha256'],'historical executable drift')
        return folder
