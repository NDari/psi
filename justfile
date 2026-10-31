# Psi's local commands. `just` lists them.

set shell := ["bash", "-euo", "pipefail", "-c"]

# llama-server settings for llm-up; override like `just ctx=65536 llm-up qwen`.
port := "9931"
ctx := "131072"
state := justfile_directory() / ".llm"

# The reference models (D-29) and each model card's sampling.
qwen := "-hf bartowski/Qwen_Qwen3.5-4B-GGUF:Q4_K_M --temp 0.7 --top-p 0.8 --top-k 20 --min-p 0 --presence-penalty 1.5 --repeat-penalty 1.0"
gemma := "-hf google/gemma-4-12B-it-qat-q4_0-gguf:Q4_0 --temp 1.0 --top-p 0.95 --top-k 64 --min-p 0"

# List the recipes
default:
    @just --list --unsorted

# The local check (D-33): format, lint, test, eval smoke. Run before every commit.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    just eval-smoke

# The eval pipeline through podman with the replay server as the model (no GPU)
eval-smoke:
    cargo test -q -p psi-cli --test eval_smoke -- --ignored --test-threads 1

# Format the code
fmt:
    cargo fmt --all

# Build the psi binary
build:
    cargo build

# Check every grader: the reference passes, the untouched repo fails
eval-check *args: build
    ./target/debug/psi eval check {{args}}

# One trial per quick-set task (needs llm-up); extra args go to psi eval run
eval-run *args: build
    ./target/debug/psi eval run --port {{port}} {{args}}

# Show an eval trial or session as a timeline
inspect path *args: build
    ./target/debug/psi inspect {{path}} {{args}}

# Start llama-server with a reference model (qwen or gemma) in the background
llm-up model="qwen":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{model}}" in
      qwen) args="{{qwen}}" ;;
      gemma) args="{{gemma}}" ;;
      *) echo "llm-up: model is qwen or gemma, not {{model}}" >&2; exit 1 ;;
    esac
    mkdir -p "{{state}}"
    if [[ -f "{{state}}/pid" ]] && kill -0 "$(cat "{{state}}/pid")" 2>/dev/null; then
      echo "llama-server already running (pid $(cat "{{state}}/pid")); run just llm-down first" >&2
      exit 1
    fi
    if curl -sf "http://127.0.0.1:{{port}}/health" >/dev/null; then
      echo "something else answers on port {{port}}; stop it first" >&2
      exit 1
    fi
    cmd="llama-server $args --no-mmproj --jinja --alias local -c {{ctx}} -ngl 99 --port {{port}} --reasoning off --seed 0"
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
