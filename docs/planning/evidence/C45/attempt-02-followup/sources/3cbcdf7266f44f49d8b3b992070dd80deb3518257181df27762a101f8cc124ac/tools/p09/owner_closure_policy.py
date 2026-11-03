"""Explicit, baseline-bound C30 owner closure; never changes a raw verdict.

Administrative exception authorized 2026-11-02. This module does not run tests,
launch a review, change a ledger, or grant C31/C32/P09 a review exception.
"""
from pathlib import Path
import subprocess
import hashlib
import phase_inputs as original

NOTE = 'docs/planning/reviews/C30/owner-override-2026-11-02.md'
ATTEMPT = 'docs/planning/reviews/C30/attempt-12'
SCORES = dict(architecture=9, behavior=9, authority_security=9, validation=8, overall=8)
PINNED = {
    NOTE: '52b64aa1fecaff18059fd62bb301448869833412fa1b81c293cb2b7a20e153d6',
    ATTEMPT + '/verdict-returned.json': 'f49d633701dcb7d606ca176f3edaeee8d435866e1ee0d142c540029c3035d2ec',
    ATTEMPT + '/source-inventory.json': 'bcab06311acc001e3a5b430b128c09d20868295016342d405dfd830a9677b11c',
    ATTEMPT + '/request.json': '940ebc477172fab9d078c458cb88646cbf1ce62de0f82b1e83c0e9ab8e75326e',
    ATTEMPT + '/receipt.json': '5885351796da2cc7623510393573f4771077c4de24b0cb26336c2439fb106ee8',
    ATTEMPT + '/post-review-audit.json': '00f40540847cceb52ffad681d59b72aa0bf64cc51446ab540282ec28346ac97b',
}
INPUT_DIGEST = 'f5f26271b8823df8a2b5299417146784d345d4e04f8b59341706e6a41750210d'
EXECUTABLE_SHA = '2791b5a1eaf6b9268e0574502c62a20f3027fd2b76ff9ed08289eef1f56147ea'


def verify_c30_owner_closure(root):
    """Return honest member metadata only for the owner's exact closed baseline."""
    root = Path(root).resolve()
    require, read, sha = original.require, original.read, original.sha
    for name, digest in PINNED.items():
        require((root / name).is_file() and sha(root / name) == digest,
                'C30 owner closure binding changed: ' + name)
    path = root / ATTEMPT / 'verdict-returned.json'
    latest, verdict = original.latest_verdict(root, 'C30')
    require(latest == path, 'C30 owner closure does not cover a later verdict')
    request = read(path.parent / 'request.json')
    receipt = read(path.parent / 'receipt.json')
    audit = read(path.parent / 'post-review-audit.json')
    require(verdict['subject'] == 'C30' and verdict['attempt_id'] == 'attempt-12'
            and verdict['scores'] == SCORES and verdict['recommendation'] == 'fail'
            and verdict['blockers'] == [], 'wrong raw C30 verdict for owner closure')
    require(verdict['request_id'] == request['request_id'] == receipt['request_id']
            and verdict['fingerprints'] == request['fingerprints'] == receipt['fingerprints']
            and request['attempt_id'] == 'attempt-12' and request['subject'] == 'C30',
            'C30 request/receipt/fingerprint mismatch')
    require(receipt['verdict_sha256'] == sha(path)
            and receipt['request_sha256'] == sha(path.parent / 'request.json')
            and receipt['post_review_audit_sha256'] == sha(path.parent / 'post-review-audit.json')
            and receipt['scores'] == SCORES and receipt['recommendation'] == 'fail'
            and receipt['blockers'] == [] and receipt['d034_accepted'] is True
            and receipt['full_gate_pass'] is False and receipt['source_frozen'] is True,
            'C30 original acceptance binding changed')
    require(audit['raw_verdict_sha256'] == sha(path)
            and audit['request_sha256'] == sha(path.parent / 'request.json')
            and audit['scores'] == SCORES and audit['d034_eligible'] is True
            and audit['compiled_inputs_unchanged'] is True and audit['full_gate_pass'] is False,
            'C30 original audit binding changed')
    inventory = read(path.parent / 'source-inventory.json')
    require(set(inventory['files']) == set(request['scope']['files']),
            'C30 owner closure source membership changed')
    aggregate = hashlib.sha256(''.join(f'{name} {digest}\n' for name, digest in sorted(inventory['files'].items())).encode()).hexdigest()
    require(aggregate == inventory['source_fingerprint'] == request['fingerprints']['source'],
            'C30 frozen source aggregate/request fingerprint mismatch')
    for name, digest in inventory['files'].items():
        require(sha(root / name) == digest, 'C30 owner closure reviewed input drift: ' + name)
    require(verdict['fingerprints']['dependencies'] == INPUT_DIGEST,
            'C30 owner closure compiled inputs changed')
    prep = read(root / 'docs/planning/evidence/P09/native-prepare.json')
    build = read(root / 'docs/planning/evidence/P09/native-build-inputs.json')
    executable = root / prep['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2'
    require(prep['executable_sha256'] == EXECUTABLE_SHA and sha(executable) == EXECUTABLE_SHA
            and build['inputs']['before_digest'] == build['inputs']['after_digest'] == INPUT_DIGEST
            and build['inputs']['unchanged'] is True,
            'C30 owner closure applies only to unchanged G7 application baseline')
    require(subprocess.run(['git', 'merge-base', '--is-ancestor',
                           '4578d2f509ce09bf2f929ce6b77dd66136b06ac1', 'HEAD'],
                          cwd=root, capture_output=True).returncode == 0,
            'C30 acceptance commit is not integrated')
    return {'subject': 'C30', 'record': str(path.relative_to(root)), 'sha256': sha(path),
            'request': ATTEMPT + '/request.json', 'request_sha256': sha(path.parent / 'request.json'),
            'receipt': ATTEMPT + '/receipt.json', 'receipt_sha256': sha(path.parent / 'receipt.json'),
            'audit': ATTEMPT + '/post-review-audit.json', 'audit_sha256': sha(path.parent / 'post-review-audit.json'),
            'owner_decision': NOTE, 'owner_decision_sha256': sha(root / NOTE),
            'scores': dict(SCORES), 'recommendation': 'fail', 'full_gate_pass': False,
            'owner_closed': True,
            'standing': 'complete by explicit owner decision; actual raw 9/9/9/8/8 unchanged; '
                        'C30 R1/R2/E1/E2 closed for this exact baseline; no new capture or score claimed'}
