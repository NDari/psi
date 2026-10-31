# calc

A tiny integer calculator: `build/calc 7 '*' 6` prints `42`.

- `src/main.c`: argument handling.
- `src/util.c`, `src/util.h`: the arithmetic.
- `tests/test_util.c`: unit tests for util.c; exits non-zero on failure.
