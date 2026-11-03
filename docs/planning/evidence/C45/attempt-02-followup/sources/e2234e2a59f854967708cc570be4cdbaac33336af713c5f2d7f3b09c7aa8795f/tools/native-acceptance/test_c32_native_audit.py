import copy
import json
from pathlib import Path
import tempfile
import unittest
from c32_native_audit import PREFIX, report
from test_c32_coverage import complete


class NativeAggregation(unittest.TestCase):
    def setUp(self):
        self.work=tempfile.TemporaryDirectory();self.addCleanup(self.work.cleanup);self.root=Path(self.work.name)
        self.original=self.root/PREFIX/'native-attempt-02';self.follow=self.root/PREFIX/'native-attempt-03';(self.original/'native').mkdir(parents=True);(self.follow/'native.staging').mkdir(parents=True)
        app={'executable_sha256':'fixture','identifier':'fixture-id'}
        self.raw={'app':app,'checks':[{'id':f'C32-N0{i}','description':'native requirement','result':'pass' if i==1 else 'fail'} for i in range(1,5)]}
        obs=complete();obs['n3']['ended_announced']=False;obs['n4']['send_reached']=False
        self.write(self.original/'native-acceptance.json',self.raw);self.write(self.original/'native/observations.json',{'observations':obs})
        self.write(self.root/'keyboard.json',[{'role':'AXButton','label':'Send','focused':True}]);self.write(self.root/'ended.json',[{'subrole':'AXApplicationStatus','depth':0},{'label':'Ended','depth':1}])
        self.extra={'app':app,'keyboard':{'before':{'role':'AXTextArea','label':'Message'},'after':{'role':'AXButton','label':'Send'},'send_reached':True,'tree':'keyboard.json'},'ended_status':{'tree':'ended.json','status_regions':['Ended'],'ended_announced':True,'engine_gone':True,'saved_transcript':True,'rail_ended':True}}
        self.path=self.follow/'native.staging/followup-observations.json';self.write(self.path,self.extra)
    def write(self,path,value):path.write_text(json.dumps(value))
    def test_new_aggregate_preserves_raw_failing_results(self):
        before=(self.original/'native-acceptance.json').read_bytes();v=report(self.root)
        self.assertTrue(all(c['result']=='pass' for c in v['checks']));self.assertEqual(v['raw_results']['C32-N02'],'fail');self.assertEqual(before,(self.original/'native-acceptance.json').read_bytes())
    def test_missing_or_failed_followup_rejected(self):
        for k in ['ended_announced','engine_gone','saved_transcript','rail_ended']:
            x=copy.deepcopy(self.extra);x['ended_status'][k]=False;self.write(self.path,x)
            with self.subTest(fact=k),self.assertRaises(AssertionError):report(self.root)
    def test_wrong_bundle_rejected(self):
        self.extra['app']=dict(self.extra['app'],executable_sha256='different');self.write(self.path,self.extra)
        with self.assertRaises(AssertionError):report(self.root)
    def test_native_focused_node_required(self):
        self.write(self.root/'keyboard.json',[{'role':'AXButton','label':'Send','focused':False}])
        with self.assertRaises(AssertionError):report(self.root)
    def test_native_status_region_required(self):
        self.write(self.root/'ended.json',[{'label':'Ended','depth':0}])
        with self.assertRaises(AssertionError):report(self.root)

if __name__=='__main__':unittest.main()
