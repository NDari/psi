import unittest

import kvcache


class FakeClock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


def make(max_size):
    clock = FakeClock()
    return kvcache.Cache(clock=clock, max_size=max_size), clock


class LRU(unittest.TestCase):
    def test_evicts_oldest(self):
        c, _ = make(2)
        c.set("a", 1)
        c.set("b", 2)
        c.set("c", 3)
        self.assertNotIn("a", c)
        self.assertEqual((c.get("b"), c.get("c"), len(c)), (2, 3, 2))

    def test_get_marks_used(self):
        c, _ = make(3)
        for k in "abc":
            c.set(k, k)
        c.get("a")
        c.set("d", "d")
        self.assertEqual(sorted(k for k in "abcd" if k in c), ["a", "c", "d"])

    def test_set_existing_marks_used_and_never_evicts(self):
        c, _ = make(2)
        c.set("a", 1)
        c.set("b", 2)
        c.set("a", 10)
        self.assertEqual(c.stats()["evictions"], 0)
        self.assertEqual(len(c), 2)
        c.set("c", 3)
        self.assertEqual((c.get("a"), "b" in c), (10, False))

    def test_contains_len_delete_miss_do_not_mark_used(self):
        c, _ = make(3)
        for k in "abc":
            c.set(k, k)
        self.assertIn("a", c)
        len(c)
        c.get("zz")
        c.delete("zz")
        c.set("d", "d")
        self.assertNotIn("a", c)
        self.assertIn("b", c)

    def test_expired_dropped_before_eviction(self):
        c, clock = make(3)
        c.set("a", 1)
        c.set("b", 2, ttl=5)
        c.set("c", 3)
        clock.now += 6
        c.set("d", 4)
        self.assertEqual([k for k in "abcd" if k in c], ["a", "c", "d"])
        self.assertEqual(c.stats()["evictions"], 0)

    def test_evictions_counted(self):
        c, _ = make(1)
        for i in range(5):
            c.set(i, i)
        c.get(4)
        c.get(0)
        self.assertEqual(c.stats(), {"hits": 1, "misses": 1, "size": 1, "evictions": 4})
        c.clear()
        self.assertEqual(c.stats()["evictions"], 4)

    def test_bad_max_size(self):
        for n in (0, -2):
            with self.assertRaises(ValueError):
                kvcache.Cache(max_size=n)

    def test_unlimited_by_default(self):
        c = kvcache.Cache(clock=FakeClock())
        for i in range(500):
            c.set(i, i)
        self.assertEqual(len(c), 500)
        self.assertEqual(c.get(0), 0)
        self.assertEqual(c.stats()["evictions"], 0)


class Existing(unittest.TestCase):
    def test_ttl_and_stats(self):
        clock = FakeClock()
        c = kvcache.Cache(clock=clock)
        c.set("a", 1, ttl=10)
        c.set("b", 2)
        self.assertEqual(c.get("a"), 1)
        clock.now += 10
        self.assertIsNone(c.get("a"))
        self.assertFalse(c.delete("a"))
        st = c.stats()
        self.assertEqual((st["hits"], st["misses"], st["size"]), (1, 1, 1))
