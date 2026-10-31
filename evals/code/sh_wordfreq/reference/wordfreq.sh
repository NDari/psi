#!/usr/bin/env bash
# wordfreq.sh [-n N] [FILE]: print the N most frequent words.
n=10
while getopts n: opt; do
  case $opt in
    n) n=$OPTARG ;;
    *) echo "usage: wordfreq.sh [-n N] [FILE]" >&2; exit 2 ;;
  esac
done
shift $((OPTIND - 1))
cat -- "${@:--}" | tr -cs 'A-Za-z' '\n' | tr 'A-Z' 'a-z' | grep -v '^$' |
  LC_ALL=C sort | uniq -c | LC_ALL=C sort -k1,1nr -k2,2 | head -n "$n" | awk '{print $1, $2}'
