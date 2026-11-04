"""Read-only review transport/archive preflight, not a new review or qualification."""
from pathlib import Path
import argparse
import json
import sys
import traceback
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--root',type=Path,required=True);args=parser.parse_args()
root=args.root.resolve();sys.path.insert(0,str(root/'tools/p11'))
from review_carry import actual_review, member
source=root/'docs/planning/evidence/P11/source-history/S4'
result={'schema':'airs-desktop/p11-member-metadata-preflight/1','source_root':str(source),'metadata_only':True,'new_reviews_issued':False,'new_qualification_created':False,'members':{}}
for subject in ('C46','C47','C49'):
    try:
        request=json.loads((root/'docs/planning/reviews'/subject/'attempt-02/request.json').read_text())
        original=request['qualification']
        observed=actual_review(root,subject,'attempt-02',source_root=source,evidence=False)
        validated=member(root,subject,{'review':observed,'reason':'API retry-only scope; metadata preflight'},original,source)
        result['members'][subject]={'status':'passed','review':validated,'original_qualification':original}
        print(subject+': archived-source/transport/full-nine member validation PASS',flush=True)
    except Exception as error:
        result['members'][subject]={'status':'failed','error':str(error)}
        traceback.print_exc()
result['passed']=all(row['status']=='passed' for row in result['members'].values())
out=Path(__file__).resolve().parent/'result.json';out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2),flush=True)
raise SystemExit(0 if result['passed'] else 1)
