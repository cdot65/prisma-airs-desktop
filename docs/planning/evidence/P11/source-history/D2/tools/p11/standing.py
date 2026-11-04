"""Durable, audited P11 standing for later production promotion.

Finalization consumes completed executions and five real full-9 reviews. It
never assigns a score or completes a controller ledger. Historical validation
uses the archived source/application, not this checkout's ignored target tree,
installed application, toolchain, hostname, or current application source.
"""
from pathlib import Path
import json
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'p10'))
from candidate import bundle_hash, digest, file_identity
from historical import Historical
from qualification import bound, read, require, sha, verify as verify_coverage
from review_gate import actual_review, release_prerequisites

CHUNKS = ('C46', 'C47', 'C48', 'C49')
SUBJECTS = (*CHUNKS, 'P11')
BASE = Path('docs/planning/evidence/P11')
ACCEPTED = BASE / 'accepted'
COMPLETION = BASE / 'completion-proof.json'
LFS_PATHS = (str(ACCEPTED / 'application/Contents/MacOS/prisma-airs-desktop'),
             str(ACCEPTED / 'application/Contents/Resources/harness/airs-harness'),
             str(ACCEPTED / 'source/resources/harness/darwin-arm64.tar.gz'))


class ArchivedP11(Historical):
    """Explicit P11 archive context; historical C45 behavior is untouched."""
    def archive_path(self, name):
        path = self.root / name
        require(path.resolve().is_relative_to((self.root / ACCEPTED).resolve()),
                'historical archive escapes accepted P11 evidence')
        return path


def reference(root, path):
    root = Path(root).resolve()
    path = Path(path).resolve(strict=True)
    require(path.is_relative_to(root.resolve()), 'P11 reference escapes checkout')
    return {'path': str(path.relative_to(root)), 'sha256': sha(path)}


def identities(root, paths):
    return {str(path.relative_to(root)): file_identity(path) for path in sorted(set(paths))}


def evidence_files(root, attempts):
    paths = [p for p in (root / BASE).rglob('*')
             if (p.is_file() or p.is_symlink()) and not p.is_relative_to(root / ACCEPTED)
             and p != root / COMPLETION and '__pycache__' not in p.parts]
    for subject, attempt in attempts.items():
        paths.extend(p for p in (root / 'docs/planning/reviews' / subject / attempt).rglob('*')
                     if (p.is_file() or p.is_symlink()) and '__pycache__' not in p.parts)
    return paths


def latest_attempt(root, subject):
    attempts = [p.name for p in (root / 'docs/planning/reviews' / subject).glob('attempt-*')
                if p.is_dir() and p.name.removeprefix('attempt-').isdigit()]
    require(attempts, 'missing actual P11 member/phase review: ' + subject)
    return max(attempts, key=lambda name: int(name.removeprefix('attempt-')))


def require_archive_lfs(root):
    result = subprocess.run(['/usr/bin/git', 'check-attr', '-z', 'filter', '--', *LFS_PATHS],
                            cwd=root, capture_output=True, check=True)
    fields = result.stdout.decode().strip('\0').split('\0')
    require(len(fields) == len(LFS_PATHS) * 3
            and all(fields[i:i+3] == [name, 'filter', 'lfs'] for i, name in zip(range(0, len(fields), 3), LFS_PATHS)),
            'configure reviewed P11 archive Git LFS paths before finalization')


def audited_reviews(root, attempts, qualified, source):
    root = Path(root).resolve()
    require(set(attempts) == set(SUBJECTS), 'P11 requires exactly four member reviews and its phase review')
    reviews = {}
    for subject in SUBJECTS:
        attempt = attempts[subject]
        require(attempt == latest_attempt(root, subject), 'newer review attempt exists: ' + subject)
        request = read(root / 'docs/planning/reviews' / subject / attempt / 'request.json')
        require(request['qualification'] == qualified,
                'P11 reviews do not bind one shared qualification: ' + subject)
        reviews[subject] = actual_review(root, subject, attempt, source_root=source)
        require(all(value >= 9 for value in reviews[subject]['scores'].values()),
                'P11 promotion requires full-9 reviewed standing: ' + subject)
    phase = read(root / 'docs/planning/reviews/P11' / attempts['P11'] / 'request.json')
    require(phase['predecessors']['members'] == {name: reviews[name] for name in CHUNKS},
            'phase review did not consume these exact four audited member reviews')
    require(all(row['fingerprints']['source'] == qualified['verified']['source_inventory_sha256']
                for row in reviews.values()), 'reviews bind a foreign qualified source')
    return reviews


def verify(root):
    """Verify immutable historical standing; no current build directory needed."""
    root = Path(root).resolve()
    path = root / COMPLETION
    require(path.is_file(), 'P11 promotion requires actual accepted standing: ' + str(COMPLETION))
    record = read(path)
    require(record['schema'] == 'airs-desktop/p11-completion-proof/1'
            and record['controller_complete'] is False, 'P11 standing schema/controller claim')
    require(record['context']['path'] == str(ACCEPTED / 'historical-context.json'),
            'P11 standing context is foreign')
    history = ArchivedP11(root, record['context'])
    qualified = record['qualification']
    original = read(bound(root, {'path': qualified['record'], 'sha256': qualified['sha256']}))
    require(original['schema'] == 'airs-desktop/p11-shared-qualification/1', 'foreign P11 qualification')
    coverage = read(bound(root, original['coverage']))
    source_hash = coverage['source_inventory_sha256']
    source = history.source(source_hash)
    seal = read(bound(root, original['source']))
    require(digest(seal['files']) == source_hash, 'original P11 source seal differs')
    import proof
    result = proof.verify_coverage(root, coverage, historical=history)
    require(result == qualified['verified'], 'archived P11 raw qualification differs')
    candidate = read(bound(root, coverage['candidate']))
    from candidate import verify_candidate
    verify_candidate(root, candidate, historical=history)
    require(coverage['candidate']['sha256'] == qualified['candidate_sha256'],
            'P11 accepted candidate reference differs')
    proof.validate_execution(root, read(bound(root, original['native_execution'])), candidate)
    inventory = read(bound(root, record['evidence']))['files']
    require(inventory, 'P11 raw evidence inventory missing')
    require(set(inventory) == {str(p.relative_to(root)) for p in evidence_files(root, record['attempts'])},
            'P11 retained evidence membership differs')
    for name, identity in inventory.items():
        target = root / name
        require(target.resolve().is_relative_to(root), 'P11 evidence escapes checkout')
        require(file_identity(target) == identity,
                'P11 retained evidence drift (hydrate pinned LFS objects if needed): ' + name)
    reviews = audited_reviews(root, record['attempts'], qualified, source)
    require(reviews == record['reviews'], 'P11 audited review standing changed')
    baseline = release_prerequisites(root)
    require(record['historical_standing'] == original['historical_standing'] == baseline,
            'P11 historical baseline standing differs')
    return {'schema': 'airs-desktop/p11-promotion-prerequisites/1',
            'record': str(COMPLETION), 'sha256': sha(path),
            'qualification': qualified, 'reviews': reviews, 'controller_complete': False}


def finalize(root, qualification_path, attempts):
    """Archive only after actual reviews; refuse every existing output path.

    Large files under accepted/application and accepted/source/resources/harness
    must have their reviewed explicit Git LFS rules before the qualified seal.
    This function never runs a test, native driver, build, signing or publication.
    """
    root = Path(root).resolve()
    destination, completion = root / ACCEPTED, root / COMPLETION
    require(not destination.exists() and not completion.exists(), 'preserve existing accepted P11 archive')
    require_archive_lfs(root)
    import proof
    qualified = proof.verify(root, Path(qualification_path).resolve())
    reviews = audited_reviews(root, attempts, qualified, root)
    original = read(root / qualified['record'])
    seal = read(bound(root, original['source']))
    coverage = read(bound(root, original['coverage']))
    candidate = read(bound(root, coverage['candidate']))
    prepared = read(bound(root, candidate['prepare']))
    evidence = identities(root, evidence_files(root, attempts))
    destination.mkdir(parents=True, exist_ok=False)

    def write(name, value):
        path = destination / name
        with path.open('x') as stream:
            json.dump(value, stream, indent=2)
            stream.write('\n')
        return reference(root, path)

    source = destination / 'source'
    source.mkdir()
    for name, identity in seal['files'].items():
        original_file, archived = root / name, source / name
        require(original_file.resolve().is_relative_to(root) or original_file.is_symlink(),
                'source path escapes checkout')
        require(archived.resolve().is_relative_to(source), 'source archive path escapes destination')
        archived.parent.mkdir(parents=True, exist_ok=True)
        if identity['kind'] == 'symlink':
            archived.symlink_to(identity['target'])
        else:
            shutil.copy2(original_file, archived)
        require(file_identity(archived) == identity, 'source changed while archiving: ' + name)
    application = destination / 'application'
    shutil.copytree(root / prepared['bundle'], application, symlinks=True)
    require(bundle_hash(application) == candidate['bundle_sha256'], 'application changed while archiving')
    source_seal = write('source-seal.json', seal)
    context = write('historical-context.json', {
        'schema': 'airs-desktop/historical-qualification-context/1',
        'sources': {coverage['source_inventory_sha256']: {
            'path': str(source.relative_to(root)), 'seal': source_seal}},
        'applications': {candidate['bundle_sha256']: {
            'path': str(application.relative_to(root)),
            'executable_sha256': candidate['executable_sha256']}}})
    evidence_ref = write('evidence-inventory.json', {'files': evidence})
    record = {'schema': 'airs-desktop/p11-completion-proof/1',
              'qualification': qualified, 'attempts': attempts, 'reviews': reviews,
              'context': context, 'evidence': evidence_ref,
              'historical_standing': original['historical_standing'],
              'controller_complete': False,
              'meaning': 'Actual audited full-9 C46-C49/P11 reviews of one shared qualification. Historical source, candidate and raw outcomes remain bound; no controller ledger or current release artifact acceptance is claimed.'}
    with completion.open('x') as stream:
        json.dump(record, stream, indent=2)
        stream.write('\n')
    return verify(root)


if __name__ == '__main__':
    require(sys.argv[1:] == ['release-prerequisites'],
            'usage: standing.py release-prerequisites; finalize is an explicit API after actual reviews')
    print(json.dumps(verify(Path.cwd()), indent=2))
