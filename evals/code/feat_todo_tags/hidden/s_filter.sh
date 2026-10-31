. /hidden/lib.sh
t add "Pay rent" -p 3 --tag home --tag money
t add "Water plants" --tag home
t add "Buy milk" -p 1 --tag shop --tag home
t add "Read book"
t done 2
t list --tag home
t list --tag home --all
t list --tag home --tag money
t list --tag HOME --sort priority
t list --tag nothing
