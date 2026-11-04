"""Actual-frame containment, including retained failure geometry and negatives."""
import unittest
from acceptance_c04 import current_frame_clear

class CurrentFrameTests(unittest.TestCase):
    def setUp(self):
        self.window=dict(pos=[320,95],size=[900,600]);self.viewport=dict(pos=[320,123],size=[900,572]);self.control=dict(pos=[449,651],size=[113,44])
    def test_actual_retained_minimum_geometry_is_visible_without_a_nonexistent_fade(self):
        self.assertGreater(self.control['pos'][1]+self.control['size'][1],self.window['pos'][1]+self.window['size'][1]-60)
        self.assertTrue(current_frame_clear(self.control,self.window,self.viewport))
    def test_below_window_control_is_rejected(self):
        self.control['pos'][1]+=5;self.assertFalse(current_frame_clear(self.control,self.window,self.viewport))
    def test_horizontal_clipping_is_rejected(self):
        self.control['pos'][0]=1200;self.assertFalse(current_frame_clear(self.control,self.window,self.viewport))
    def test_viewport_clipping_is_rejected_even_inside_window(self):
        self.viewport['size'][1]-=20;self.assertFalse(current_frame_clear(self.control,self.window,self.viewport))
    def test_missing_focused_control_or_webview_cannot_pass(self):
        self.assertFalse(current_frame_clear(None,self.window,self.viewport));self.assertFalse(current_frame_clear(self.control,self.window,None))

if __name__=='__main__':unittest.main()
