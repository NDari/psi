# sales report

    python3 report.py SALES.csv [--top N]

SALES.csv has a header line `date,region,amount`. The report prints one line
per region with its total, biggest total first (regions with the same total
in alphabetical order), then a TOTAL line. `--top N` keeps only the first N
regions; the TOTAL line always covers every region.
