. /evallib/check.sh
w=$(mktemp -d)
cp -r . "$w/p"
cd "$w/p" || exit 0
rm -rf build
make >/dev/null 2>&1
check builds_calc bash -c '[ "$(./build/calc 7 "*" 6)" = 42 ]'
check objects_in_build bash -c '[ -f build/main.o ] && [ -f build/util.o ]'
check second_make_is_noop bash -c 'sleep 1; touch "$1"; sleep 1; make >/dev/null 2>&1 && [ -z "$(find build -newer "$1")" ]' _ "$w/stamp"
check touch_c_rebuilds_one bash -c 'sleep 1; touch src/util.c; touch "$1"; sleep 1; make >/dev/null 2>&1 && [ build/util.o -nt "$1" ] && [ build/calc -nt "$1" ] && [ ! build/main.o -nt "$1" ]' _ "$w/stamp2"
check touch_header_rebuilds_objects bash -c 'sleep 1; touch src/util.h; touch "$1"; sleep 1; make >/dev/null 2>&1 && [ build/util.o -nt "$1" ] && [ build/main.o -nt "$1" ]' _ "$w/stamp3"
printf '#!/bin/sh\necho "$*" >> "%s"\nexec cc "$@"\n' "$w/cc.log" > "$w/cc"
chmod +x "$w/cc"
check cc_and_cflags_used bash -c 'make clean >/dev/null 2>&1; make CC="$1/cc" CFLAGS="-O0 -DEVAL_MARKER=1" >/dev/null 2>&1 && [ -x build/calc ] && grep "EVAL_MARKER" "$1/cc.log" | grep -q "src/main.c" && grep "EVAL_MARKER" "$1/cc.log" | grep -q "src/util.c"' _ "$w"
check test_passes bash -c 'make test'
check clean_removes_build bash -c 'make >/dev/null 2>&1; make clean && [ ! -e build ] && make >/dev/null 2>&1 && [ -x build/calc ]'
cp -r "$w/p" "$w/broken"
sed -i "s/case '-': \*out = a - b;/case '-': *out = b - a;/" "$w/broken/src/util.c"
check test_fails_on_bug bash -c 'cd "$1" && make clean >/dev/null 2>&1; grep -q "b - a" src/util.c && make >/dev/null 2>&1 && ! make test >/dev/null 2>&1' _ "$w/broken"
cd /
rm -rf "$w"
