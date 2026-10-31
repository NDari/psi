. /evallib/check.sh
S=$PWD/manifest.sh
w=$(mktemp -d)
export LC_ALL=C
mkt() { # DIR: the fixture tree
  mkdir -p "$1/css" "$1/img/icons" "$1/.well-known" "$1/empty dir"
  printf '<h1>hi</h1>\n' > "$1/index.html"
  printf 'body { margin: 0 }\n' > "$1/css/site.css"
  printf 'PNG\x00\x01binary' > "$1/img/icons/logo one.png"
  printf 'contact: ops@example.com\n' > "$1/.well-known/security.txt"
  : > "$1/empty.txt"
  printf 'B\n' > "$1/B.txt"
  printf 'a b\n' > "$1/a b"
  printf 'dot\n' > "$1/a.txt"
  printf 'nested\n' > "$1/img/x"
  ln -s index.html "$1/link.html"
}
line() { printf '%s %s %s\n' "$(sha256sum < "$1/$2" | cut -d' ' -f1)" "$(stat -c %s "$1/$2")" "$2"; }
mkt "$w/site"
for p in .well-known/security.txt B.txt "a b" a.txt css/site.css empty.txt "img/icons/logo one.png" img/x index.html; do
  line "$w/site" "$p"
done > "$w/want"
bash "$S" create "$w/site" > "$w/got" 2>/dev/null
check_output create_lines "$w/want" cat "$w/got"
check create_byte_order bash -c 'cut -d" " -f3- "$1" | LC_ALL=C sort -c && [ "$(cut -d" " -f3- "$1" | head -n 4 | tr "\n" "|")" = ".well-known/security.txt|B.txt|a b|a.txt|" ]' _ "$w/got"
check create_skips_symlinks_and_dirs bash -c '[ -s "$1" ] && ! grep -q "link.html\|empty dir" "$1"' _ "$w/got"
check create_relative_dir bash -c 'cd "$1" && bash "$2" create ./site | cmp -s - "$1/want"' _ "$w" "$S"
cp "$w/want" "$w/manifest"
check verify_clean_exit_0 bash -c 'o=$(bash "$1" verify "$2/site" "$2/manifest"); [ $? -eq 0 ] && [ -z "$o" ]' _ "$S" "$w"
mkt "$w/s2"
printf '<h1>ho</h1>\n' > "$w/s2/index.html"
printf 'body { margin: 1px }\n' > "$w/s2/css/site.css"
rm "$w/s2/a b" "$w/s2/.well-known/security.txt"
printf 'new\n' > "$w/s2/new file.txt"
printf 'x\n' > "$w/s2/img/icons/extra.svg"
printf '%s\n' 'MISSING .well-known/security.txt' 'MISSING a b' 'CHANGED css/site.css' 'EXTRA img/icons/extra.svg' 'CHANGED index.html' 'EXTRA new file.txt' > "$w/want_diff"
check_output verify_report "$w/want_diff" bash -c 'bash "$1" verify "$2/s2" "$2/manifest"; true' _ "$S" "$w"
check verify_exit_1 bash -c 'bash "$1" verify "$2/s2" "$2/manifest" >/dev/null 2>&1; [ $? -eq 1 ]' _ "$S" "$w"
mkt "$w/s3"
printf '<h1>ho</h1>\n' > "$w/s3/index.html"
check_output verify_same_size_change <(echo 'CHANGED index.html') bash -c 'bash "$1" verify "$2/s3" "$2/manifest"; [ $? -eq 1 ]' _ "$S" "$w"
check usage_errors_exit_2 bash -c 'for a in "" "frobnicate $2/site" "create" "create $2/nope" "verify $2/site" "verify $2/site $2/nope.txt"; do e=$(bash "$1" $a 2>&1 >/dev/null); r=$?; [ $r -eq 2 ] && [ -n "$e" ] || { echo "args: $a: exit $r"; exit 1; }; done' _ "$S" "$w"
cd /
rm -rf "$w"
