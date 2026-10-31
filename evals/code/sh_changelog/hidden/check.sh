. /evallib/check.sh
S=$PWD/changelog.sh
w=$(mktemp -d)
export HOME=$w GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=Dev GIT_AUTHOR_EMAIL=dev@example.com GIT_COMMITTER_NAME=Dev GIT_COMMITTER_EMAIL=dev@example.com
t=1700000000
c() { # SUBJECT: commit a change with an increasing date
  t=$((t + 60))
  echo "$1" >> "f-$t"
  git add -A >/dev/null
  GIT_AUTHOR_DATE="@$t +0000" GIT_COMMITTER_DATE="@$t +0000" git commit -q -m "$1"
}
mkdir "$w/r" && cd "$w/r" || exit 0
git init -q -b main .
c "chore: initial import"
git tag v1
c "feat: add export command"
c "fix(parser): handle empty lines"
git checkout -q -b side
c "feat(ui): dark mode"
git checkout -q main
c "docs: update README"
t=$((t + 60))
GIT_AUTHOR_DATE="@$t +0000" GIT_COMMITTER_DATE="@$t +0000" git merge -q --no-ff -m "Merge branch 'side'" side
c "fix typo in help text"
c "feature: not a conventional type"
c "feat(api): paginate results"
git tag v2
c "fix: crash on exit"
c "fix(cli): wrong exit code"
h() { git log --format=%H -n 1 --grep="^$1\$" | cut -c1-7; }
{
  echo "## Features"
  echo "- add export command ($(h 'feat: add export command'))"
  echo "- ui: dark mode ($(h 'feat(ui): dark mode'))"
  echo "- api: paginate results ($(h 'feat(api): paginate results'))"
  echo
  echo "## Fixes"
  echo "- parser: handle empty lines ($(h 'fix(parser): handle empty lines'))"
  echo
  echo "## Other"
  echo "- docs: update README ($(h 'docs: update README'))"
  echo "- fix typo in help text ($(h 'fix typo in help text'))"
  echo "- feature: not a conventional type ($(h 'feature: not a conventional type'))"
} > "$w/want_v1v2"
{
  echo "## Fixes"
  echo "- crash on exit ($(h 'fix: crash on exit'))"
  echo "- cli: wrong exit code ($(h 'fix(cli): wrong exit code'))"
} > "$w/want_v2"
sect() { awk -v h="## $2" '$0 == h {on = 1; next} /^## / {on = 0} on && NF' "$1"; }
bash "$S" v1 v2 > "$w/out" 2>/dev/null
check_output full_release_notes "$w/want_v1v2" cat "$w/out"
for x in Features Fixes Other; do
  sect "$w/want_v1v2" $x > "$w/want_$x"
  sect "$w/out" $x > "$w/got_$x"
done
check_output features_with_scopes "$w/want_Features" cat "$w/got_Features"
check_output fixes_section "$w/want_Fixes" cat "$w/got_Fixes"
check_output other_section "$w/want_Other" cat "$w/got_Other"
check merges_excluded bash -c 'grep -q "^- " "$1" && ! grep -q Merge "$1"' _ "$w/out"
check_output empty_sections_left_out "$w/want_v2" bash "$S" v2 HEAD
check_output to_defaults_to_head "$w/want_v2" bash "$S" v2
check bad_revision_exit_1 bash -c 'for args in "v1 nosuchref" "nosuchtag"; do e=$(bash "$1" $args 2>&1 >/dev/null); r=$?; [ $r -eq 1 ] && [ -n "$e" ] || exit 1; done' _ "$S"
cd /
rm -rf "$w"
