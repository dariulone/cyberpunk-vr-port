import unittest
from hud_gate import validates


class StartupGateTests(unittest.TestCase):
    def test_true_startup_arms(self):
        self.assertTrue(validates(0, (1, 72, 10, 0, 10), 0))
        self.assertTrue(validates(1, (1, 72, 10, 10, 0), 1))

    def test_runtime_flip_is_not_startup_evidence(self):
        self.assertFalse(validates(0, (1, 72, 10, 10, 0), 0))
        self.assertFalse(validates(1, (1, 72, 10, 0, 10), 1))
        self.assertFalse(validates(0, (2, 144, 20, 10, 10), 0))

    def test_unobserved_or_irrelevant_copy_is_not_applied(self):
        self.assertFalse(validates(0, (0, 0, 0, 0, 0), 0))
        self.assertFalse(validates(1, (1, 72, 0, 0, 0), 1))
        self.assertFalse(validates(1, (1, 72, 10, 0, 0), 1))


if __name__ == '__main__':
    unittest.main()
