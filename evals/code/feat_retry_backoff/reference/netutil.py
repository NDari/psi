"""Small helpers for talking to the status service over HTTP."""
import json
import time
import urllib.error
import urllib.request


class TransientError(Exception):
    """A failure that may go away if the request is tried again."""


class PermanentError(Exception):
    """A failure that will not go away by trying again."""


RETRYABLE_STATUS = {429, 502, 503, 504}


def fetch_json(url, timeout=10.0, opener=urllib.request.urlopen):
    """GET url and decode the JSON body.

    Raises TransientError for timeouts, connection problems and the status
    codes in RETRYABLE_STATUS, and PermanentError for other HTTP errors and
    bodies that are not JSON.
    """
    try:
        with opener(url, timeout=timeout) as resp:
            body = resp.read()
    except urllib.error.HTTPError as e:
        if e.code in RETRYABLE_STATUS:
            raise TransientError(f"{url}: HTTP {e.code}") from e
        raise PermanentError(f"{url}: HTTP {e.code}") from e
    except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
        raise TransientError(f"{url}: {e}") from e
    try:
        return json.loads(body)
    except ValueError as e:
        raise PermanentError(f"{url}: body is not JSON") from e


def retry(func, *, attempts=3, base_delay=0.5, factor=2.0, max_delay=10.0,
          retry_on=(TransientError,), sleep=time.sleep):
    """Call func() until it succeeds, waiting longer after each failure."""
    if attempts < 1:
        raise ValueError(f"attempts must be at least 1, got {attempts}")
    delay = base_delay
    for attempt in range(1, attempts + 1):
        try:
            return func()
        except retry_on:
            if attempt == attempts:
                raise
        sleep(min(delay, max_delay))
        delay *= factor


def service_status(base_url, opener=urllib.request.urlopen):
    """Return the "status" field of BASE_URL/status."""
    data = fetch_json(base_url.rstrip("/") + "/status", opener=opener)
    return data.get("status", "unknown")
