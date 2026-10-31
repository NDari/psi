. /evallib/check.sh
unittests /hidden/test_env.py
check_output cli_uses_environment /hidden/want_cli.txt env MAILER_SERVER_PORT=9100 MAILER_SMTP_USE_TLS=off MAILER_QUEUE_DIR=/data/spool python3 settings.py /hidden/mailer.ini
