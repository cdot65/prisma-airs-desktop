import hashlib,json
from pathlib import Path
root=Path('/Users/cdot/development/redtail/prisma-airs-desktop-v2').resolve()
def read(relative):
 p=(root/relative).resolve()
 assert p.is_relative_to(root)
 return p.read_bytes()
def digest(relative):
 return hashlib.sha256(read(relative)).hexdigest()
records=json.loads(read('docs/planning/evidence/academy-2.1/validation/run.json'))['commands'][-3:]
for record in records:
 assert record['exit_code']==0 and record['build_inputs_unchanged']
 assert digest(record['log'])==record['sha256']
 for path,expected in record['build_inputs'].items():
  assert digest(path)==expected,path
 if 'test_executable' in record:
  binary=record['test_executable']
  assert digest(binary['path'])==binary['sha256']
for path,expected in json.loads(read('docs/planning/reviews/academy-2.1/attempt-06/inputs.json')).items():
 assert digest(path)==expected,path
summary=json.loads(read('docs/planning/evidence/academy-2.1/hosted/summary.json'))
for receipt in summary['receipts'] + summary.get('additional_artifacts', []):
 assert digest(receipt['path'])==receipt['sha256']
print(json.dumps({'read_only_audit':'passed','validation_records':len(records),'bound_build_inputs':len(records[-1]['build_inputs']),'hosted_receipt_hashes':len(summary['receipts'])}))
