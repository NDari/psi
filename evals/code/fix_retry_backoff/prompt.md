Our retry helper in retry.py misbehaves. With retry(fn, attempts=3), fn is only called twice, and when every call fails retry() returns None instead of raising the error. Also the backoff doesn't grow: the log shows waits of 0.5s, 1.0s, 1.0s, 1.0s instead of 0.5s, 1s, 2s, 4s.

The docstring of retry() describes what it should do. Please fix it; the signature retry(fn, attempts=3, base_delay=0.5, max_delay=8.0, retry_on=(Exception,), sleep=time.sleep) stays the same.
