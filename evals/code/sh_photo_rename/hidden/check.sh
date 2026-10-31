. /evallib/check.sh
work=$(mktemp -d)
D="$work/photos dir"
mkdir -p "$D/sub"
echo img > "$D/IMG_20230514_103012.jpg"
echo pxl > "$D/PXL_20221231_235959123.jpg"
echo shot > "$D/Screenshot_20240102_080910.PNG"
echo hol > "$D/Holiday IMG_20230601_120000.Jpeg"
echo dup > "$D/IMG_20200101_000000.jpg"
echo existing > "$D/2020-01-01_00-00-00.jpg"
echo notes > "$D/notes.txt"
echo short > "$D/IMG_2023051_103012.jpg"
echo subimg > "$D/sub/IMG_20190909_090909.jpg"
bash rename_photos.sh "$D" > "$work/out" 2> "$work/err"
cd "$D" || exit 0
check renames_img bash -c '[ -f 2023-05-14_10-30-12.jpg ] && [ "$(cat 2023-05-14_10-30-12.jpg)" = img ] && [ ! -e IMG_20230514_103012.jpg ]'
check renames_pxl_first_match bash -c '[ "$(cat 2022-12-31_23-59-59.jpg 2>/dev/null)" = pxl ] && [ ! -e PXL_20221231_235959123.jpg ]'
check lowercases_extension bash -c '[ "$(cat 2024-01-02_08-09-10.png 2>/dev/null)" = shot ] && [ ! -e 2024-01-02_08-09-10.PNG ]'
check name_with_spaces bash -c '[ "$(cat 2023-06-01_12-00-00.jpeg 2>/dev/null)" = hol ] && [ ! -e "Holiday IMG_20230601_120000.Jpeg" ]'
check others_untouched bash -c '[ "$(cat notes.txt)" = notes ] && [ "$(cat IMG_2023051_103012.jpg)" = short ] && [ "$(cat sub/IMG_20190909_090909.jpg)" = subimg ] && [ ! -e 2019-09-09_09-09-09.jpg ] && [ ! -e sub/2019-09-09_09-09-09.jpg ]'
check collision_skipped bash -c '[ "$(cat IMG_20200101_000000.jpg 2>/dev/null)" = dup ] && [ "$(cat 2020-01-01_00-00-00.jpg)" = existing ] && grep -qx "skip: IMG_20200101_000000.jpg" "$1"' _ "$work/err"
printf '%s\n' 'Holiday IMG_20230601_120000.Jpeg -> 2023-06-01_12-00-00.jpeg' 'IMG_20230514_103012.jpg -> 2023-05-14_10-30-12.jpg' 'PXL_20221231_235959123.jpg -> 2022-12-31_23-59-59.jpg' 'Screenshot_20240102_080910.PNG -> 2024-01-02_08-09-10.png' > "$work/want"
check_output stdout_lines "$work/want" cat "$work/out"
cd /
rm -rf "$work"
