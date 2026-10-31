Write `logreport.sh`, run as `bash logreport.sh FILE`, that summarizes a web server access log in combined log format (there's a sample in logs/access.log). Blank lines are ignored. The output must be exactly:
```
requests: <number of requests>
unique ips: <number of distinct client IPs>
bytes: <sum of the response size field; a size of "-" counts as 0>
status:
  <code>: <count>        (one line per status code, codes in ascending order)
top paths:
  <count> <path>         (the 3 most requested paths, most first; ties in alphabetical order)
```
The path is the request path without its query string (`/search?q=x` counts as `/search`). If there are fewer than 3 distinct paths, list them all. Lines under `status:` and `top paths:` are indented by two spaces.
