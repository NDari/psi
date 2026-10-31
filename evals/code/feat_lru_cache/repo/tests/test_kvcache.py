import unittest

from kvcache import Cache


class FakeClock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


class CacheTest(unittest.TestCase):
    def setUp(self):
        self.clock = FakeClock()
        self.cache = Cache(clock=self.clock)

    def test_set_get(self):
        self.cache.set("a", 1)
        self.assertEqual(self.cache.get("a"), 1)
        self.assertIsNone(self.cache.get("b"))
        self.assertEqual(self.cache.get("b", 7), 7)

    def test_expiry(self):
        self.cache.set("a", 1, ttl=10)
        self.clock.now += 9.9
        self.assertIn("a", self.cache)
        self.clock.now += 0.1
        self.assertNotIn("a", self.cache)
        self.assertEqual(len(self.cache), 0)

    def test_stats(self):
        self.cache.set("a", 1)
        self.cache.get("a")
        self.cache.get("x")
        st = self.cache.stats()
        self.assertEqual((st["hits"], st["misses"], st["size"]), (1, 1, 1))

    def test_delete(self):
        self.cache.set("a", 1)
        self.assertTrue(self.cache.delete("a"))
        self.assertFalse(self.cache.delete("a"))


if __name__ == "__main__":
    unittest.main()
