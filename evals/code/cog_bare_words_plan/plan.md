# Bare words run what they are bound to

## Goal

A bare word bound to a block runs the block. Today it pushes the block and
needs an explicit `apply`:

```
{ dup * } :square let
5 square apply          (today: 25)
5 square                (today: 5 {dup *}; wanted: 25)
#[ a: {dup *} ] :x let
5 x.a                   (today: 5 {dup *}; wanted: 25)
5 incr                  (today: 5 {1 +}; wanted: 6)
```

## What to know

- Cog is a stack language interpreter in Go, in this directory. Build with
  `go build -o cog ./cmd/cog`. Try an input with
  `echo '5 incr' | ./cog` (it prints the stack, then `OK`).
- The check is `go build ./... && go test ./...`. It must pass at the end
  of every step, so each step also fixes the tests it breaks.
- There is no network. The Go modules are in `vendor/`; don't run
  `go get` or `go mod tidy`.
- Words are evaluated in `env.go`, in `evalWord`. For a name the user
  bound it looks the value up (`e.lookup`, or `e.lookupMember` for a
  dotted name like `x.a`) and pushes it. That push is what changes.
- `apply` already knows how to run a block. Find how it does it (search
  `stdlib.go` and `fn.go` for `apply`) and run the bound value the same
  way, so stack-effect checks and error positions keep working.
- Only a value that `apply` would run is run. A number, a string, a dict
  or any other value bound to a name is pushed as before.
- `$name` is a delayed reference: it pushes the bound value without
  running it. It must keep doing that; it is now the way to get a block
  onto the stack by name.
- `apply` itself stays, for a block that is on the stack: `5 {dup *} apply`.
- Many tests, examples and docs write `name apply` for a bound block.
  After this change that runs the block twice, or fails. Where a test
  breaks for that reason, the fix is to remove the `apply` (or write
  `$name apply`), not to change what the test checks.

## Steps

- [ ] Plain names. In `evalWord` in `env.go`, when a name without a dot is bound to a block, run the block instead of pushing it. Add tests in a new file `bare_words_test.go`: `{ dup * } :square let 5 square` leaves 25; `$square` still pushes the block; a name bound to a number still pushes the number. Then run `go test ./...` and fix every test that fails because it wrote `name apply` for a plain name, until it passes.
- [ ] Dotted names. Do the same in `evalWord` for a dotted name (the `e.lookupMember` branch): a dict field or a module member bound to a block runs. Add tests to `bare_words_test.go`: `#[ a: {dup *} ] :x let 5 x.a` leaves 25, and a field holding a number is still pushed. Run `go test ./...` and fix the tests that wrote `x.a apply`.
- [ ] The standard library and the REPL. Build with `go build -o cog ./cmd/cog` and check `echo '5 incr' | ./cog` prints `6 OK` and `echo '#[ a: {dup *}] :x let 5 x.a' | ./cog` prints `25 OK`. If either does not, find why (words defined in `stdlib.go`, or how `cmd/cog/repl.go` evaluates a line) and fix it. Add a test for `5 incr` leaving 6.
- [ ] Examples and docs. Update `README.md`, the files in `docs/` and the `.cog` files in `examples/` where they show `name apply` for a bound block, so they show the bare name. Say in `README.md` that a bare name runs a bound block and `$name` pushes it. `go test ./...` must still pass (it runs the examples).
