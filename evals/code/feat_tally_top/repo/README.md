# tally

Word counts for text files.

    python3 tally.py notes.txt
    cat *.txt | python3 tally.py --ignore-case

Output is one `COUNT WORD` line per word, most frequent first, ties in
alphabetical order.
