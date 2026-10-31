import unittest

import netutil


class Flaky:
    """Fails with the given exceptions in turn, then returns "ok"."""

    def __init__(self, *errors):
        self.errors = list(errors)
        self.calls = 0

    def __call__(self):
        self.calls += 1
        if self.errors:
            raise self.errors.pop(0)
        return "ok"


class Retry(unittest.TestCase):
    def setUp(self):
        self.slept = []

    def test_success_first_try(self):
        f = Flaky()
        self.assertEqual(netutil.retry(f, sleep=self.slept.append), "ok")
        self.assertEqual((f.calls, self.slept), (1, []))

    def test_succeeds_on_third_attempt(self):
        f = Flaky(netutil.TransientError("a"), netutil.TransientError("b"))
        self.assertEqual(netutil.retry(f, sleep=self.slept.append), "ok")
        self.assertEqual(f.calls, 3)
        self.assertEqual(self.slept, [0.5, 1.0])

    def test_all_attempts_fail_reraises_last(self):
        errs = [netutil.TransientError(str(i)) for i in range(4)]
        f = Flaky(*errs)
        with self.assertRaises(netutil.TransientError) as cm:
            netutil.retry(f, attempts=4, base_delay=1, factor=3, sleep=self.slept.append)
        self.assertIs(cm.exception, errs[3])
        self.assertEqual(f.calls, 4)
        self.assertEqual(self.slept, [1, 3, 9])

    def test_delay_capped(self):
        f = Flaky(*[netutil.TransientError() for _ in range(5)])
        out = netutil.retry(f, attempts=6, base_delay=2, factor=2, max_delay=5, sleep=self.slept.append)
        self.assertEqual(out, "ok")
        self.assertEqual(self.slept, [2, 4, 5, 5, 5])

    def test_other_exception_propagates_at_once(self):
        f = Flaky(netutil.PermanentError("no"), netutil.TransientError())
        with self.assertRaises(netutil.PermanentError):
            netutil.retry(f, attempts=5, sleep=self.slept.append)
        self.assertEqual((f.calls, self.slept), (1, []))

    def test_custom_retry_on(self):
        f = Flaky(KeyError("k"), OSError("o"))
        self.assertEqual(netutil.retry(f, retry_on=(KeyError, OSError), base_delay=0.1,
                                       sleep=self.slept.append), "ok")
        self.assertEqual(f.calls, 3)
        self.assertEqual(len(self.slept), 2)
        f = Flaky(netutil.TransientError())
        with self.assertRaises(netutil.TransientError):
            netutil.retry(f, retry_on=(KeyError,), sleep=self.slept.append)

    def test_attempts_below_one(self):
        f = Flaky()
        for n in (0, -1):
            with self.assertRaises(ValueError):
                netutil.retry(f, attempts=n, sleep=self.slept.append)
        self.assertEqual(f.calls, 0)

    def test_single_attempt_no_sleep(self):
        f = Flaky(netutil.TransientError("x"))
        with self.assertRaises(netutil.TransientError):
            netutil.retry(f, attempts=1, sleep=self.slept.append)
        self.assertEqual((f.calls, self.slept), (1, []))


class Existing(unittest.TestCase):
    def test_fetch_json_still_works(self):
        import io
        import urllib.error

        def ok(url, timeout):
            return io.BytesIO(b'{"status": "green"}')

        def busy(url, timeout):
            raise urllib.error.HTTPError(url, 503, "busy", {}, None)

        self.assertEqual(netutil.service_status("http://x/", opener=ok), "green")
        with self.assertRaises(netutil.TransientError):
            netutil.fetch_json("http://x/status", opener=busy)
