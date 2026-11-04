"""Dated unchanged-impact carry of three actual full-nine P11 member reviews.

No review is reissued, rescored or relabelled current. C48 and the phase must
review the current qualification; the phase also reviews this carry logic.
"""
from pathlib import Path

import proof
from review_gate import actual_review

read, bound, require, sha, digest = proof.read, proof.bound, proof.require, proof.sha, proof.digest
CARRIED = ('C46', 'C47', 'C49')
OLD_QUALIFICATION = 'docs/planning/evidence/P11/qualification-final-03/qualification.json'
OLD_QUALIFICATION_SHA = '21eb4c44168ce7ce5cd3e01484399fba397b788176e310e5d15df696cebbb218'
SOURCE = Path('docs/planning/evidence/P11/source-history/S4')
APPLICATION = Path('docs/planning/evidence/P11/review-carry-baseline-01/application')
# Set only after the three-file retry fix freezes. Other production changes
# must not borrow this one-off decision.
RETRY_DELTA = {'apps/desktop/src/v2/store.ts': ('8bc2feec3d7de4c6dc8ef31ab6233c88b8209218285917de0c50acd1e70225d8',
                                  '4612a821dc4779c4af4751ab426d227bcf8458f34e7a71f0ff4137f027850fee'),
 'apps/desktop/src/v2/workbench/api-client.test.tsx': ('161ad953f06049d464ef001857c3cb59fb221e62ff8d9e255c7a75a99d1cb548',
                                                       'cf397af69ee6f19b399c64c475aa44d749f3121c8b217816c0d838b585516672'),
 'apps/desktop/src/v2/workbench/api-client.tsx': ('b95eb3953ff1e51b16012d38c982b4f71cebc39b12c294b383c2134346d43bde',
                                                  '3116c93e0d83bb997a1edda763223be43b99888e04d90c42ababea574975923b')}


def impact_delta(before, after, declared):
    changed = sorted(n for n in set(before) | set(after) if before.get(n) != after.get(n))
    require(RETRY_DELTA and changed == declared, 'review carry source delta missing/undeclared')
    for name in changed:
        if name in RETRY_DELTA:
            require(tuple(files.get(name, {}).get('sha256') for files in (before, after)) == RETRY_DELTA[name],
                    'review carry is not the frozen retry-only fix')
        else:
            require(name.startswith('tools/p11/') or name in (
                '.gitattributes', 'docs/planning/refinements/2026-11-04/execution.md'),
                'unlisted behavior/configuration change cannot carry reviews: ' + name)
    require(set(RETRY_DELTA) <= set(changed), 'retry fix source membership missing')
    return changed


def dependencies(files):
    roots = ('apps/desktop/', 'crates/desktop-core/', 'crates/xtask/src/',
             'packages/desktop-contracts/', 'resources/', 'docs/architecture/', '.cargo/')
    exact = {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'pnpm-lock.yaml',
             'pnpm-workspace.yaml', 'package.json', 'crates/xtask/Cargo.toml'}
    return sorted(n for n in files if (n in exact or n.startswith(roots)) and n not in RETRY_DELTA)


def unchanged(before, after, names, identities):
    needed = dependencies(after)
    require(names == needed and needed and set(identities) == set(needed)
            and all(identities[n] == before.get(n) == after.get(n) for n in needed),
            'carried review dependency identity changed/missing')


def member(root, subject, entry, original_qualification, source_root):
    original = entry['review']
    require(subject in CARRIED and original['subject'] == subject
            and original['attempt'] == 'attempt-02' and entry['reason'], 'unapproved carried member/subject')
    folder = root/'docs/planning/reviews'/subject/original['attempt']
    request = read(folder/'request.json')
    require(request['subject'] == subject and request['qualification'] == original_qualification,
            'original member reviewed another qualification')
    # The actual audit/transport and archived source are rechecked. Historical
    # qualification/source/artifact proofs suffice; do not recursively execute
    # the old global consumer against newly pinned remediation constants.
    actual = actual_review(root, subject, original['attempt'], source_root=source_root, evidence=False)
    require(actual == original and len(actual['scores']) == 5
            and all(value >= 9 for value in actual['scores'].values()), 'original full-nine review differs')
    return actual


def qualification_bindings(record, qualified):
    require(record['schema'] == 'airs-desktop/p11-member-review-carry/1'
            and set(record['members']) == set(CARRIED)
            and record['current_qualification'] == qualified
            and qualified['verified']['qualified'] is True, 'review carry subject/current qualification differs')
    original = record['original_qualification']
    require(original['record'] == OLD_QUALIFICATION and original['sha256'] == OLD_QUALIFICATION_SHA,
            'review carry original qualification differs')
    return original


def validate(root, path, qualified, source_root=None):
    record = read(path)
    original = qualification_bindings(record, qualified)
    old_record = read(bound(root, {'path': original['record'], 'sha256': original['sha256']}))
    current_record = read(bound(root, {'path': qualified['record'], 'sha256': qualified['sha256']}))
    require(record['original_source'] == old_record['source']
            and record['current_source'] == current_record['source'], 'review carry source references differ')
    before = read(bound(root, record['original_source']))['files']
    after = read(bound(root, record['current_source']))['files']
    require(digest(before) == original['verified']['source_inventory_sha256']
            and digest(after) == qualified['verified']['source_inventory_sha256'], 'review carry source fingerprints differ')
    archived = root/SOURCE
    require(record['original_source_archive'] == str(SOURCE)
            and {str(p.relative_to(archived)): proof.file_identity(p) for p in archived.rglob('*')
                 if p.is_file() or p.is_symlink()} == before, 'original reviewed source archive differs')
    current = (proof.source_inventory(root) if source_root is None or source_root == root else
        {str(p.relative_to(source_root)): proof.file_identity(p) for p in source_root.rglob('*')
         if p.is_file() or p.is_symlink()})
    require(current == after, 'current reviewed source differs')
    impact_delta(before, after, record['changed_paths'])
    unchanged(before, after, record['unchanged_dependencies'], record['file_identities'])
    old_coverage = read(bound(root, old_record['coverage']))
    old_candidate = read(bound(root, old_coverage['candidate']))
    require(old_coverage['candidate']['sha256'] == original['candidate_sha256'], 'original reviewed candidate differs')
    preservation = read(bound(root, record['application_preservation']))
    app = root/APPLICATION
    require(record['original_application'] == str(APPLICATION)
            and preservation['application'] == str(APPLICATION)
            and preservation['qualification'] == {'path': original['record'], 'sha256': original['sha256']}
            and preservation['bundle_sha256'] == old_candidate['bundle_sha256']
            and preservation['executable_sha256'] == old_candidate['executable_sha256']
            and proof.bundle_hash(app) == old_candidate['bundle_sha256']
            and sha(app/'Contents/MacOS/prisma-airs-desktop') == old_candidate['executable_sha256'],
            'original reviewed application was not preserved')
    return {'members': {subject: member(root, subject, record['members'][subject], original, archived)
                        for subject in CARRIED},
            'proof': proof.reference(root, path), 'original_source': archived,
            'original_qualification': original, 'record': record}
