My phone exports photos with names like `IMG_20230514_103012.jpg`, `PXL_20221231_235959123.jpg` or `Screenshot_20240102_080910.PNG`. Please write `rename_photos.sh`, run as `bash rename_photos.sh DIR`, that renames them by date.

- Look only at regular files directly inside DIR (not in subdirectories). A file qualifies if its name contains 8 digits, an underscore and 6 digits (`YYYYMMDD_HHMMSS`); use the first such match.
- Rename it within DIR to `YYYY-MM-DD_HH-MM-SS.EXT`, where EXT is the original extension (the part after the last dot) in lowercase.
- For each rename, print `OLD -> NEW` (just the file names) on stdout, in byte order of the old names.
- If the new name already exists, leave the file as it is and print `skip: OLD` to stderr.
- Files that don't qualify are left alone. Names may contain spaces.
