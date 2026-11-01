# Psi's local commands. `just` lists them.

set shell := ["bash", "-euo", "pipefail", "-c"]

# llama-server settings for llm-up; override like `just ctx=65536 llm-up qwen`.
port := "9931"
# "auto" is 131072, or 65536 for qwen35b (the context it was first run with).
ctx := "auto"
# Speculative decoding (llama-server --spec-type): "auto" is draft-mtp for
# qwen, whose GGUF carries its own draft layers (about 1.8x faster
# generation here), and none for gemma, which has none. `just spec=none llm-up qwen`.
spec := "auto"
# One state directory per port, so two servers can run side by side.
state := justfile_directory() / ".llm" / port

# The reference models (D-29) and each model card's sampling.
qwen := "-hf bartowski/Qwen_Qwen3.5-4B-GGUF:Q4_K_M --temp 0.7 --top-p 0.8 --top-k 20 --min-p 0 --presence-penalty 1.5 --repeat-penalty 1.0"
gemma := "-hf google/gemma-4-12B-it-qat-q4_0-gguf:Q4_0 --temp 1.0 --top-p 0.95 --top-k 64 --min-p 0"
# A larger model, not yet a reference model: the owner's command on the Mac,
# with thinking on and kept in the history. Sampling is the server's default.
qwen35b := '-hf unsloth/Qwen3.6-35B-A3B-MTP-GGUF:UD-Q4_K_XL -fa on -np 1 --spec-draft-n-max 3 --chat-template-kwargs {"preserve_thinking":true}'

# List the recipes
default:
    @just --list --unsorted

# The local check (D-33): format, lint, test, eval smoke. Run before every commit.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    just eval-smoke

# The static psi the eval trials run: the host's glibc can be newer than the image's
eval-build:
    cargo build -q --target x86_64-unknown-linux-musl -p psi-cli

eval-binary := justfile_directory() / "target/x86_64-unknown-linux-musl/debug/psi"

# The eval pipeline through podman with the replay server as the model (no GPU)
eval-smoke: eval-build
    env PSI_EVAL_BINARY={{eval-binary}} cargo test -q -p psi-cli --test eval_smoke -- --ignored --test-threads 1

# Regenerate docs/api.md and docs/api/psi.lua from the extension API definition
api-docs:
    env PSI_BLESS=1 cargo test -q -p psi-ext-api --test drift

# Lint an extension
ext-check path:
    cargo run -q -- ext check {{path}}

# Run an extension's *_test.lua files (default: every example)
ext-test path="examples/extensions":
    cargo run -q -- ext test {{path}}

# Format the code
fmt:
    cargo fmt --all

# Build the psi binary
build:
    cargo build

# Build an optimized psi (target/release/psi)
release:
    cargo build --release

# Build an optimized psi and install it as ~/.local/bin/psi
install: release
    install -Dm755 target/release/psi "$HOME/.local/bin/psi"
    @echo "installed $HOME/.local/bin/psi ($("$HOME/.local/bin/psi" --version))"

# Run psi in the directory you call this from, e.g. `just run -p "hi"`
[no-cd]
[positional-arguments]
run *args:
    cargo run -q --manifest-path {{justfile_directory()}}/Cargo.toml -p psi-cli -- "$@"

# The same with the optimized build
[no-cd]
[positional-arguments]
run-release *args:
    cargo run -q --release --manifest-path {{justfile_directory()}}/Cargo.toml -p psi-cli -- "$@"

# Check every grader: the reference passes, the untouched repo fails
eval-check *args: build eval-build
    ./target/debug/psi eval check --psi-binary {{eval-binary}} {{args}}

# One trial per quick-set task (needs llm-up); extra args go to psi eval run
eval-run *args: build eval-build
    ./target/debug/psi eval run --port {{port}} --psi-binary {{eval-binary}} {{args}}

# The extension-writing tasks (H-01); pick some with `just ext-tasks=ext_todo_scan eval-ext`
ext-tasks := "ext_lockfile_guard,ext_trim_output,ext_todo_scan,ext_bookmarks"
# Their step cap: passes took 7 to 16 steps, so 20 cuts the time failures waste (`just ext-steps=40 eval-ext`)
ext-steps := "20"

# The extension-writing eval (H-01): the ext_* tasks, with docs/extensions.md and docs/api.md in the model's context
eval-ext *args: build eval-build
    mkdir -p target/eval-ext-docs
    cat docs/extensions.md docs/api.md > target/eval-ext-docs/AGENTS.md
    ./target/debug/psi eval run --port {{port}} --psi-binary {{eval-binary}} --psi-config target/eval-ext-docs --tasks {{ext-tasks}} --steps {{ext-steps}} {{args}}

# Show an eval trial or session as a timeline
inspect path *args: build
    ./target/debug/psi inspect {{path}} {{args}}

# Start llama-server with a model (qwen, gemma or qwen35b) in the background
llm-up model="qwen":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{model}}" in
      qwen) args="{{qwen}}"; auto_spec=draft-mtp; auto_ctx=131072; reasoning=off ;;
      gemma) args="{{gemma}}"; auto_spec=none; auto_ctx=131072; reasoning=off ;;
      qwen35b) args='{{qwen35b}}'; auto_spec=draft-mtp; auto_ctx=65536; reasoning=auto ;;
      *) echo "llm-up: model is qwen, gemma or qwen35b, not {{model}}" >&2; exit 1 ;;
    esac
    spec="{{spec}}"
    [[ "$spec" == auto ]] && spec=$auto_spec
    ctx="{{ctx}}"
    [[ "$ctx" == auto ]] && ctx=$auto_ctx
    mkdir -p "{{state}}"
    if [[ -f "{{state}}/pid" ]] && kill -0 "$(cat "{{state}}/pid")" 2>/dev/null; then
      echo "llama-server already running (pid $(cat "{{state}}/pid")); run just llm-down first" >&2
      exit 1
    fi
    if curl -sf "http://127.0.0.1:{{port}}/health" >/dev/null; then
      echo "something else answers on port {{port}}; stop it first" >&2
      exit 1
    fi
    cmd="llama-server $args --no-mmproj --jinja --alias local -c $ctx -ngl 99 --port {{port}} --reasoning $reasoning --seed 0 --spec-type $spec"
    echo "$cmd" > "{{state}}/cmd"
    # setsid is util-linux's; macOS has none, and nohup alone keeps it running.
    detach="nohup"
    command -v setsid >/dev/null && detach="setsid nohup"
    $detach $cmd > "{{state}}/log" 2>&1 < /dev/null &
    echo $! > "{{state}}/pid"
    echo "llama-server starting ({{model}}), log: {{state}}/log"
    for _ in $(seq 600); do
      if curl -sf "http://127.0.0.1:{{port}}/health" >/dev/null; then
        echo "ready on http://127.0.0.1:{{port}}"
        exit 0
      fi
      if ! kill -0 "$(cat "{{state}}/pid")" 2>/dev/null; then
        echo "llama-server exited; last log lines:" >&2
        tail -n 20 "{{state}}/log" >&2
        rm -f "{{state}}/pid"
        exit 1
      fi
      sleep 1
    done
    echo "not ready after 10 minutes; see {{state}}/log" >&2
    exit 1

# Stop the llama-server that llm-up started
llm-down:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -f "{{state}}/pid" ]]; then echo "not running"; exit 0; fi
    pid=$(cat "{{state}}/pid")
    if kill "$pid" 2>/dev/null; then
      for _ in $(seq 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
      echo "stopped llama-server (pid $pid)"
    else
      echo "not running"
    fi
    rm -f "{{state}}/pid"
