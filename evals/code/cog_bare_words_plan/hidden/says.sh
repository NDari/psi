# says.sh INPUT WANT: the cog REPL answers INPUT with a line starting WANT.
printf '%s\n' "$1" | go run ./cmd/cog 2>&1 | grep -q "^$2"
