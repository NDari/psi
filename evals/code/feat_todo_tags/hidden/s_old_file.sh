. /hidden/lib.sh
cp /hidden/old.json "$TODO_FILE"
t list --all
t tag 2 work
t list
t tags
t add "New one" --tag work
t list --tag work
