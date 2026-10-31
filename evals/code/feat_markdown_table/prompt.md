Please add a `to_markdown(rows, columns)` function to report.py, next to `to_csv` and `to_text`. It returns a Markdown table as one string:

- a header line with the column names, then a separator line, then one line per row in the given order;
- every line is `"| " + " | ".join(cells) + " |"` with no padding, and lines end with "\n" (including the last one);
- the separator cell is `---`, or `---:` for a numeric column. A column is numeric when it has at least one non-empty value and every non-empty value in it parses with `float()`;
- a missing key or a None value is an empty cell, other values go through `str()`, and a `|` inside a value or a column name is written as `\|`.
