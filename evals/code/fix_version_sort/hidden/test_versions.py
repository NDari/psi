import unittest

import versions


class TestVersions(unittest.TestCase):
    def test_compare_minor(self):
        self.assertEqual(versions.compare("1.10.0", "1.9.0"), 1)
        self.assertEqual(versions.compare("1.9.0", "1.10.0"), -1)

    def test_compare_major_and_patch(self):
        self.assertEqual(versions.compare("10.0", "9.9.9"), 1)
        self.assertEqual(versions.compare("1.0.2", "1.0.10"), -1)
        self.assertEqual(versions.compare("v2.0.11", "2.0.9"), 1)

    def test_latest(self):
        self.assertEqual(versions.latest(["v2.0", "v10.1"]), "v10.1")
        self.assertEqual(versions.latest(["0.9", "0.10", "0.2.30"]), "0.10")

    def test_sort_versions(self):
        got = versions.sort_versions(["1.10", "1.2", "1.9.1", "10.0", "2.0", "1.2.10", "1.2.9"])
        self.assertEqual(got, ["1.2", "1.2.9", "1.2.10", "1.9.1", "1.10", "2.0", "10.0"])

    def test_trailing_zeros_after_fix(self):
        self.assertEqual(versions.compare("1.10", "1.10.0"), 0)
        self.assertEqual(versions.compare("1.10.0.0", "1.9.99"), 1)
        self.assertEqual(versions.latest(["3.10.0", "3.9"]), "3.10.0")

    def test_existing_behaviour(self):
        self.assertEqual(versions.compare("v1.2", "1.2.0"), 0)
        self.assertEqual(versions.compare("1.2.1", "1.2"), 1)
        for bad in ["1.x", "", "1..2", "v"]:
            with self.assertRaises(ValueError, msg=bad):
                versions.parse_version(bad)


if __name__ == "__main__":
    unittest.main()
