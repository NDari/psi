"""Retry with exponential backoff, for calls to flaky services."""
import logging
import time

log = logging.getLogger(__name__)


def retry(fn, attempts=3, base_delay=0.5, max_delay=8.0, retry_on=(Exception,), sleep=time.sleep):
    """Call fn() until it succeeds, at most `attempts` times in all.

    After the k-th failed call (k = 1, 2, ...) wait base_delay * 2**(k-1)
    seconds, but never more than max_delay, then try again. There is no
    wait after the last attempt. Exceptions not in retry_on propagate at
    once. When every attempt fails, the last exception is raised.
    """
    if attempts < 1:
        raise ValueError("attempts must be 1 or more")
    delay = base_delay
    for attempt in range(1, attempts):
        try:
            return fn()
        except retry_on as e:
            if attempt == attempts:
                raise
            log.warning("attempt %d/%d failed: %s; retrying in %.1fs", attempt, attempts, e, delay)
            sleep(delay)
            delay = min(base_delay * 2, max_delay)


def retrying(**options):
    """Decorator form: @retrying(attempts=5) def call(...): ..."""
    def wrap(fn):
        def wrapper(*args, **kwargs):
            return retry(lambda: fn(*args, **kwargs), **options)
        wrapper.__name__ = fn.__name__
        wrapper.__doc__ = fn.__doc__
        return wrapper
    return wrap
