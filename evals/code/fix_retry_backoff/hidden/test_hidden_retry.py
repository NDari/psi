import unittest

from retry import retry, retrying


class Flaky:
    """Fails `failures` times with ConnectionError, then returns "ok"."""

    def __init__(self, failures):
        self.failures = failures
        self.calls = 0

    def __call__(self):
        self.calls += 1
        if self.calls <= self.failures:
            raise ConnectionError(f"boom {self.calls}")
        return "ok"


class TestHiddenRetry(unittest.TestCase):
    def setUp(self):
        self.sleeps = []

    def run_retry(self, fn, **kw):
        return retry(fn, sleep=self.sleeps.append, **kw)

    def test_calls_attempts_times(self):
        fn = Flaky(99)
        with self.assertRaises(ConnectionError):
            self.run_retry(fn, attempts=3)
        self.assertEqual(fn.calls, 3)

    def test_raises_last_error(self):
        fn = Flaky(99)
        with self.assertRaises(ConnectionError) as cm:
            self.run_retry(fn, attempts=4)
        self.assertEqual(str(cm.exception), "boom 4")

    def test_success_on_last_attempt(self):
        fn = Flaky(2)
        self.assertEqual(self.run_retry(fn, attempts=3), "ok")
        self.assertEqual(fn.calls, 3)
        self.assertEqual(self.sleeps, [0.5, 1.0])

    def test_single_attempt(self):
        fn = Flaky(99)
        with self.assertRaises(ConnectionError):
            self.run_retry(fn, attempts=1)
        self.assertEqual(fn.calls, 1)
        self.assertEqual(self.sleeps, [])

    def test_delays_double(self):
        with self.assertRaises(ConnectionError):
            self.run_retry(Flaky(99), attempts=5)
        self.assertEqual(self.sleeps, [0.5, 1.0, 2.0, 4.0])

    def test_delays_capped(self):
        with self.assertRaises(ConnectionError):
            self.run_retry(Flaky(99), attempts=6, base_delay=1.0, max_delay=5.0)
        self.assertEqual(self.sleeps, [1.0, 2.0, 4.0, 5.0, 5.0])

    def test_decorator(self):
        fn = Flaky(3)

        @retrying(attempts=4, base_delay=2.0, sleep=self.sleeps.append)
        def call():
            return fn()

        self.assertEqual(call(), "ok")
        self.assertEqual(self.sleeps, [2.0, 4.0, 8.0])

    def test_existing_behaviour(self):
        fn = Flaky(0)
        self.assertEqual(self.run_retry(fn), "ok")
        self.assertEqual((fn.calls, self.sleeps), (1, []))
        calls = []

        def bad():
            calls.append(1)
            raise KeyError("x")

        with self.assertRaises(KeyError):
            self.run_retry(bad, attempts=5, retry_on=(ConnectionError,))
        self.assertEqual(len(calls), 1)
        with self.assertRaises(ValueError):
            self.run_retry(Flaky(0), attempts=0)


if __name__ == "__main__":
    unittest.main()
