"""One request from an access log."""
from dataclasses import dataclass
from datetime import datetime


@dataclass(frozen=True)
class Entry:
    host: str
    time: datetime  # always timezone-aware
    method: str
    path: str  # without the query string
    status: int
    size: int  # response bytes; 0 when the log has "-"

    @property
    def status_class(self):
        """"2xx", "3xx", ... for the status code."""
        return f"{self.status // 100}xx"
