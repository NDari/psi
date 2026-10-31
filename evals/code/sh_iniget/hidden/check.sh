. /evallib/check.sh
F=/hidden/test.ini
s() { bash iniget.sh "$@"; }
out() { # NAME WANT ARGS...: stdout equals WANT plus newline and exit 0
  local name=$1 want=$2; shift 2
  check_output "$name" <(printf '%s\n' "$want") bash iniget.sh "$@"
}
out simple_value example.org $F server host
out trims_whitespace 9000 $F server port
out value_with_equals 'user=app password=x=y' $F server dsn
out last_one_wins second $F server mode
out other_section 30 $F client timeout
check commented_key_not_found bash -c 'o=$(bash iniget.sh /hidden/test.ini server timeout); r=$?; [ $r -eq 1 ] && [ -z "$o" ]'
check case_sensitive_key bash -c 'o=$(bash iniget.sh /hidden/test.ini client retries); r=$?; [ $r -eq 1 ] && [ -z "$o" ]'
check missing_file_exit_2 bash -c 'bash iniget.sh /nonexistent/x.ini server host; [ $? -eq 2 ]'
