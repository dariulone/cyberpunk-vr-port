import unittest
from analyze import summarize,segments
class AnalysisTests(unittest.TestCase):
 def test_submissions_are_not_delivery(self):
  r={'window_s':2,'events':{'accepted_real':90,'accepted_synthetic':90,'fallback_repeat':20},'compositor':{'estimate_fps':60,'zero_present_records':30}}
  s=summarize([r]);self.assertEqual(s['accepted_synthetic_fps'],45);self.assertEqual(s['app_repeat_fps'],10);self.assertEqual(s['compositor_estimate_fps'],60)
 def test_no_fake_delivery(self):
  s=summarize([{'window_s':1,'events':{'accepted_synthetic':90}}]);self.assertIsNone(s['compositor_estimate_fps']);self.assertEqual(s['compositor_coverage_seconds'],0)
 def test_transitions_kept_separate(self):
  a={'window_s':1,'settings':{'enabled':True},'effective_backend':1,'events':{}}
  b={**a,'events':{'settings_change':2}}
  self.assertEqual(len(segments([a,b,a])),3)
 def test_weighted(self):
  s=summarize([{'window_s':1,'compositor':{'estimate_fps':90}},{'window_s':3,'compositor':{'estimate_fps':50}}]);self.assertEqual(s['compositor_estimate_fps'],60)
if __name__=='__main__':unittest.main()
