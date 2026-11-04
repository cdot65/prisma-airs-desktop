import copy
import runpy
import unittest
from pathlib import Path
from c32_coverage import BOOLEAN_FACTS, MOTION, problems


def complete():
    o = {g: {f: True for f in fs} for g, fs in BOOLEAN_FACTS.items()}
    o['n2'].update(guard_initial_focus='Stay', guard_buttons=['Stay', 'Discard and switch', 'Archive and switch'])
    o['n3'].update(writes_after_staging=0, writes_after_review=0)
    o['n4'].update(reduce_motion={f: True for f in MOTION}, zoom={t: {'overflow_px': 0, 'horizontal_scrollbars': 0} for t in ('light', 'dark')}, ring={'focused': 'New investigation', 'diff': 200})
    return o


class Coverage(unittest.TestCase):
    def test_complete(self): self.assertEqual(problems(complete()), [])
    def test_every_missing_or_false_fact_rejected(self):
        for g, fs in BOOLEAN_FACTS.items():
            for f in fs:
                for value in (None, False):
                    with self.subTest(group=g, fact=f, value=value):
                        o=complete(); o[g][f]=value; self.assertTrue(problems(o))
    def test_motion_each_required(self):
        for f in MOTION:
            o=complete();del o['n4']['reduce_motion'][f];self.assertTrue(problems(o))
    def test_guard_layout_ring_and_write_negative(self):
        for group, field, value in [('n2','guard_initial_focus',''),('n2','guard_buttons',[]),('n3','writes_after_review',1),('n3','writes_after_staging',None),('n4','zoom',{}),('n4','ring',{})]:
            o=complete();o[group][field]=value;self.assertTrue(problems(o))
    def test_native_mapping_requires_original_assertions(self):
        a=runpy.run_path(str(Path(__file__).with_name('legacy_assertions.py')))['ASSERTIONS']
        for n in ('02','03','04'):
            cid='C32-N'+n
            self.assertIn(cid,a[cid][1])

if __name__ == '__main__': unittest.main()
