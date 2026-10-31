# logkeeper

Housekeeping for application logs. `archive_logs.sh` runs nightly from cron:

    archive_logs.sh -d 14 /var/log/myapp /srv/archive/myapp
