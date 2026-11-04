All 6 medium/high items done. Here's a summary of everything I fixed:

---

## Issues Fixed (HIGH + MEDIUM)

### Issue 1 - HIGH: `bracketsBalanced` REPL bug
**Files changed:** `cmd/cog/repl.go`, `cmd/cog/repl_test.go`, `cmd/cog/editor_test.go`

- **Bug:** The `[` case in `bracketsBalanced` returned `true` immediately for bare `[` (not `#[`), meaning incomplete input like `"1 ["` was sent to the parser instead of waiting for `]`.
- **Fix:** Removed the early-return `if i == 0 || input[i-1] != '#'` check. Now `[` increments `depth` just like `{` and `(`. Changed return from `depth <= 0` to `depth == 0` so extra closing brackets (`] ]`) are also rejected.
- **Test:** Added `TestBracketsBalanced` with 16 cases covering: bare brackets, nested brackets, unbalanced cases, `#[` dictionary syntax, string-escaped brackets, and mixed delimiters.
- **Also fixed:** `TestRemovedListDelimiterDoesNotWaitForContinuation` in `editor_test.go` was asserting the old buggy behavior (`assert.True` for `"[1"`); changed to `assert.False` since bare brackets must now be balanced.

### Issue 2 - MEDIUM: Flaky terminal test
**File:** No change needed - `TestTerminalEditBeforeSubmissionAndCancel` passes consistently now. The earlier failure was caused by the cached build running with the old (buggy) code. Verified with `-count=5`.

### Issue 3 - MEDIUM: `swap` error wrapping `%v` → `%w`
**Files changed:** `fn.go`, `error_test.go`
- **Bug:** `swap` used `fmt.Errorf("...%v", err)` instead of `%w`, breaking the error chain so `errors.As` couldn't unwrap to the inner error.
- **Fix:** Changed both error lines to use `%w`.
- **Test:** Added `TestSwapErrorWrapsInnerError` verifying the error chains through to a `*userError` via `errors.As`.

### Issue 4 - MEDIUM: Dead error path in `dup`
**File changed:** `fn.go`
- **Bug:** `stackNotInExpectedShape(e, "dup", 1)` guarantees ≥1 stack item, so `pop()` can never fail. The `if err != nil` block after pop was dead code.
- **Fix:** Replaced `top, err := e.pop(); if err != nil { ... }` with `top, _ := e.pop()`.

### Issue 5 - MEDIUM: Discarded `pop()` errors in `nip` and `say`
**File changed:** `fn.go`
- **Bug:** Both `nip` and `say` used `top, _ := e.pop()`, silently dropping the error. Safe behind guards but misleading to readers.
- **Fix:** Changed to `top, popErr := e.pop()` with explicit `if popErr != nil` return, adding a comment explaining the guard makes this unreachable but handling it defensively to avoid nil dereference downstream.

### Issue 6 - MEDIUM: Duplicate test numbers in `lexer_test.go`
**File changed:** `lexer_test.go`
- **Bug:** Two entries labeled test 7 (`{7, NUM, "3.0023"}` and `{7, NUM, "22.1"}`), and test 15 appeared twice (`{15, NUM, "2"}` and `{15, EOF, ""}`). This makes failure output confusing.
- **Fix:** Renumbered sequentially: 7→8 for `"22.1"`, 15→18 for `EOF`.
