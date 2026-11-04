"""Actual P11 finalization producer. Execute only after the phase audit passes."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path('/Users/cdot/development/redtail/p09-queue/refinement-20261104/core').resolve()
QUALIFICATION = ROOT / 'docs/planning/evidence/P11/qualification-final-04/qualification.json'
CARRY = ROOT / 'docs/planning/evidence/P11/member-review-carry-01/receipt.json'
REVIEWED_SOURCE = '9153adb8f1cdb5cf381d2c45a25df5f88b2e04e0'
SOURCE_INVENTORY = '74569d24e8bbbaf1da407dba48cd6d3794e34e3c23315bb63dbd0abb86951a57'
ATTEMPTS = {'C46': 'attempt-02', 'C47': 'attempt-02', 'C48': 'attempt-03',
            'C49': 'attempt-02', 'P11': 'attempt-01'}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def write(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')

def main():
    if sys.argv[1:]:
        raise ValueError('No arguments accepted; fixed qualified input and actual review map only')
    if digest(QUALIFICATION) != 'e34765810f58717392047ffe1a252628534dd5bec213fc1a6775016e93f64231':
        raise ValueError('qualification04 bytes changed')
    if digest(CARRY) != 'eb25c3c5883955cf2ea754d0e701a93e80a3202c96cd8656ca54f13c23723b6e':
        raise ValueError('member review carry bytes changed')
    if digest(ROOT / 'tools/p11/review.py') != '82083687b3c25dd2a46632495fd58f617f1f5eb14f75a5451cebd168cbd15a03':
        raise ValueError('review consumer source changed')
    if subprocess.check_output(['/usr/bin/git', 'rev-parse', REVIEWED_SOURCE + '^{commit}'],
                               cwd=ROOT, text=True).strip() != REVIEWED_SOURCE:
        raise ValueError('reviewed source commit object missing')
    original = json.loads(QUALIFICATION.read_text())
    coverage = json.loads((ROOT / original['coverage']['path']).read_text())
    if coverage['source_inventory_sha256'] != SOURCE_INVENTORY:
        raise ValueError('qualification current source inventory changed')
    os.chdir(ROOT)
    sys.path[:0] = [str(ROOT / 'tools/p11'), str(ROOT / 'tools/p10')]
    import standing
    if (ROOT / standing.ACCEPTED).exists() or (ROOT / standing.COMPLETION).exists():
        raise ValueError('preserve existing accepted output or completion proof')
    standing.require_archive_lfs(ROOT)
    seal = json.loads((ROOT / original['source']['path']).read_text())
    candidate = json.loads((ROOT / coverage['candidate']['path']).read_text())
    prepare = json.loads((ROOT / candidate['prepare']['path']).read_text())
    source_bytes = sum((ROOT / name).stat().st_size for name, identity in seal['files'].items()
                       if identity['kind'] != 'symlink')
    bundle_bytes = sum(path.stat().st_size for path in (ROOT / prepare['bundle']).rglob('*')
                       if path.is_file() and not path.is_symlink())
    free_bytes = shutil.disk_usage(ROOT).free
    if free_bytes < source_bytes + bundle_bytes + 256 * 1024 * 1024:
        raise ValueError('insufficient archive disk space including256MiB reserve')
    started = datetime.now(timezone.utc).isoformat()
    result = standing.finalize(ROOT, QUALIFICATION, ATTEMPTS)
    # finalize already performs historical verification; this explicit second
    # consumption verifies only the completed archive, not app/native checks.
    post = standing.verify(ROOT)
    if post != result:
        raise ValueError('post-archive standing differs from finalization result')
    accepted = ROOT / standing.ACCEPTED
    producer = accepted / 'standing-producer.py'
    with producer.open('xb') as stream:
        stream.write(Path(__file__).read_bytes())
    promotion = accepted / 'promotion-prerequisites.json'
    write(promotion, post)
    receipt = {
        'schema': 'airs-desktop/p11-standing-producer/1',
        'started_utc': started,
        'finished_utc': datetime.now(timezone.utc).isoformat(),
        'argv': [sys.executable, str(Path(__file__).resolve())],
        'cwd': str(ROOT), 'exit_code': 0,
        'producer': {'actual_private_path': str(Path(__file__).resolve()),
                     'path': str(producer.relative_to(ROOT)), 'sha256': digest(producer)},
        'qualification': {'path': str(QUALIFICATION.relative_to(ROOT)), 'sha256': digest(QUALIFICATION)},
        'review_carry': {'path': str(CARRY.relative_to(ROOT)), 'sha256': digest(CARRY)},
        'attempts': ATTEMPTS,
        'reviewed_source_commit': REVIEWED_SOURCE,
        'source_inventory_sha256': SOURCE_INVENTORY,
        'accepted_git_commit': None,
        'accepted_commit_meaning': 'Identified after commit in a private accepted-source handoff; no future commit self-reference.',
        'archive_space': {'source_bytes': source_bytes, 'bundle_bytes': bundle_bytes,
                          'free_bytes_at_admission': free_bytes},
        'post_archive_verified': True,
        'promotion': {'path': str(promotion.relative_to(ROOT)), 'sha256': digest(promotion)},
        'controller_complete': False,
        'meaning': 'Actual audited P11 standing only; production/live acceptance and publication remain separate.'}
    write(accepted / 'standing-producer-execution.json', receipt)
    print(json.dumps({'standing': result, 'producer_receipt':
          str((accepted / 'standing-producer-execution.json').relative_to(ROOT))}, indent=2))

if __name__ == '__main__':
    main()
