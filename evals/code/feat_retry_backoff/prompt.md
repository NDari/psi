The status service is flaky, so netutil.py needs a retry helper. Add

    retry(func, *, attempts=3, base_delay=0.5, factor=2.0, max_delay=10.0,
          retry_on=(TransientError,), sleep=time.sleep)

It calls `func()` (no arguments) and returns its result. When the call raises an exception that is an instance of one of `retry_on`, it sleeps and tries again, up to `attempts` calls in total. The waits are base_delay, base_delay*factor, base_delay*factor**2, ..., each capped at max_delay, and each wait is one call to `sleep(seconds)`. It never sleeps after the last attempt: when every attempt fails, it re-raises the last exception. Any other exception propagates at once without sleeping. `attempts` below 1 raises ValueError before func is called.
