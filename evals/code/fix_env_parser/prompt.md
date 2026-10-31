Two bugs in envfile.py, our .env reader (format in README.md):

1. DATABASE_URL=postgres://app:secret@localhost:5432/app?sslmode=disable comes back as "postgres://app:secret@localhost:5432/app?sslmode".
2. SMTP_PASSWORD="s3cret # not a comment" comes back with the part after the # cut off (and a stray quote), though a # inside quotes is supposed to be kept.

See .env.example for both. Please fix them without changing the rest of the format: parse(text) and load(path) should keep returning a dict.
