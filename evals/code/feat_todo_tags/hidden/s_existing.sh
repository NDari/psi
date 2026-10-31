. /hidden/lib.sh
t add "Pay rent" -p 1 --due tomorrow
t add "Water plants"
t add "Old thing" --due 2026-10-20
t edit 2 --title "Water the plants" -p 3
t done 1
t list
t list --all --sort due
t rm 3
t rm 3
t list --all
