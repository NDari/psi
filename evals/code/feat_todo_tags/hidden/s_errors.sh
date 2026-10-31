. /hidden/lib.sh
t add "Pay rent" --tag ok --tag "two words"
t list
t add "Pay rent" --tag=-dash
t add "Pay rent" --tag home
t tag 1 good Bad_Tag
t tag 9 home
t untag 9 home
t list
