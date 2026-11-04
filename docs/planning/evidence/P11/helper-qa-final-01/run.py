from pathlib import Path
import json,sys,unittest
root=Path.cwd()
suite=unittest.TestSuite()
for directory,pattern in [
 ('tools/p11','test_*.py'),
 ('tools/native-acceptance','test_acceptance_p11_api.py'),
 ('tools/native-acceptance','test_acceptance_p11_roles.py'),
 ('tools/native-acceptance','test_acceptance_p11_credentials.py'),
 ('tools/native-acceptance','test_acceptance_p11_geometry.py')]:
 suite.addTests(unittest.TestLoader().discover(str(root/directory),pattern=pattern))
result=unittest.TextTestRunner(verbosity=1).run(suite)
summary={'passed':result.testsRun-len(result.failures)-len(result.errors)-len(result.skipped),
         'failed':len(result.failures)+len(result.errors),'skipped':len(result.skipped)}
Path(__file__).with_name('counts.json').write_text(json.dumps(summary,indent=2)+'\\n')
raise SystemExit(0 if result.wasSuccessful() else 1)
