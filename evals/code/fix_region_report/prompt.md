report.py is supposed to rank regions by total sales, biggest first (see README.md), but `python3 report.py sales.csv --top 3` shows the three smallest regions instead of the three biggest.

Please fix it so the regions come out biggest total first, regions with equal totals in alphabetical order, with --top N keeping the N biggest. The output format and the TOTAL line (always the sum over all regions) should stay as they are.
