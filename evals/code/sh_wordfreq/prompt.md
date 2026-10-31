I need a quick word-frequency tool. Write `wordfreq.sh`, run as `bash wordfreq.sh [-n N] [FILE]`.

- A word is a maximal run of ASCII letters (a-z, A-Z); everything else separates words, so "don't" is the two words "don" and "t". Words are counted case-insensitively and printed in lowercase.
- Print the N most frequent words (default 10), one per line as `COUNT WORD` (the count, one space, the word), most frequent first; words with the same count are in alphabetical order. If there are fewer than N distinct words, print them all.
- With no FILE, read standard input.

There's a sample text in samples/.
