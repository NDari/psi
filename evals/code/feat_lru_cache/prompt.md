The cache in kvcache.py grows without bound. Add a size limit with least-recently-used eviction:

- `Cache(clock=time.monotonic, max_size=None)`: None means unlimited (as now); a max_size below 1 raises ValueError.
- When `set` stores a new key and the cache already holds max_size entries, it first drops expired entries; if it is still full, it evicts the least recently used live entry. Replacing the value of an existing key never evicts anything.
- "Used" means a `get` that hits, or a `set` of that key. `in`, `len()`, `delete` and misses do not change the order.
- `stats()` gets a new key "evictions": how many entries were evicted for space (expired entries dropped do not count). `clear()` leaves the counters alone.

Keep the existing behavior and the visible tests in tests/ (run with python3 -m unittest) working.
