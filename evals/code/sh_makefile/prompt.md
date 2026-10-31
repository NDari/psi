This little C calculator has no build file yet. Please write a `Makefile` (GNU make) with:

- `all` (the default target): builds `build/calc` from `src/main.c` and `src/util.c`. Each object is compiled to `build/NAME.o` by one pattern rule `build/%.o: src/%.c`, and every object also depends on `src/util.h`.
- `test`: builds `build/test_util` from `tests/test_util.c` and `build/util.o`, then runs it; `make test` must fail if the tests fail.
- `clean`: removes `build/`.
- Use `$(CC)` and `$(CFLAGS)` for compiling (default `CFLAGS = -Wall -O2`, overridable from the command line), and create `build/` when needed.

Running `make` twice in a row must not rebuild anything the second time, and touching one `.c` file must rebuild only its object and what links it.
