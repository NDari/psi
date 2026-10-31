# backup-tools

`rotate_backups.sh DIR KEEP` is run from cron after the nightly backup. It
keeps the KEEP most recent `backup-YYYY-MM-DD.tar.gz` files in DIR and
deletes the older ones.
