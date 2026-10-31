"""Settings for logsift, in one place."""
import dataclasses
import json


@dataclasses.dataclass
class Config:
    max_line_length: int = 400
    time_format: str = "%Y-%m-%d %H:%M:%S"
    slow_ms: int = 500
    top_n: int = 5


def load_config(path):
    """Read a JSON object with any subset of Config's fields."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    known = {f.name for f in dataclasses.fields(Config)}
    unknown = sorted(set(data) - known)
    if unknown:
        raise ValueError(f"unknown config keys: {', '.join(unknown)}")
    return Config(**data)
