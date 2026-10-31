CC ?= cc
CFLAGS = -Wall -O2

OBJS = build/main.o build/util.o

.PHONY: all test clean

all: build/calc

build/calc: $(OBJS)
	$(CC) $(CFLAGS) -o $@ $(OBJS)

build/%.o: src/%.c src/util.h
	@mkdir -p build
	$(CC) $(CFLAGS) -c -o $@ $<

build/test_util: tests/test_util.c build/util.o src/util.h
	@mkdir -p build
	$(CC) $(CFLAGS) -o $@ tests/test_util.c build/util.o

test: build/test_util
	./build/test_util

clean:
	rm -rf build
