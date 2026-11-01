"""Generates evals/code/long_api_docs (repo, reference, hidden); output is committed.

Run from the repository root: python3 -I evals/code/long_api_docs/gen.py evals/code/long_api_docs
(delete the old repo/, reference/ and hidden/ first)."""
import os, random, sys

root = sys.argv[1]
rnd = random.Random(7)

MODULES = {
    "billing": ("invoice", "charge", "refund", "discount", "tax", "credit", "ledger", "payment"),
    "inventory": ("stock", "sku", "warehouse", "batch", "reorder", "shelf", "count", "supplier"),
    "shipping": ("parcel", "route", "carrier", "label", "zone", "pickup", "tracking", "rate"),
    "customers": ("account", "contact", "address", "segment", "note", "tier", "consent", "profile"),
    "reports": ("summary", "chart", "export", "period", "metric", "trend", "column", "filter"),
    "auth": ("token", "session", "role", "password", "grant", "scope", "login", "key"),
    "search": ("query", "index", "term", "ranking", "facet", "synonym", "snippet", "page"),
    "notify": ("email", "message", "template", "channel", "digest", "alert", "queue", "receipt"),
}
VERBS = ("load", "save", "build", "check", "format", "merge", "split", "count", "find", "update",
         "parse", "render", "apply", "clear", "list", "close", "open", "sort", "group", "score")
ARGS = ("items", "value", "limit", "name", "rows", "key", "when", "amount", "codes", "size")

def body(rnd, args):
    a = args[0]
    kinds = [
        f"    result = []\n    for x in {a}:\n        if x:\n            result.append(x)\n    return result\n",
        f"    total = 0\n    for x in {a}:\n        total += len(str(x))\n    return total\n",
        f"    if not {a}:\n        return None\n    return str({a}).strip().lower()\n",
        f"    seen = {{}}\n    for x in {a}:\n        seen[x] = seen.get(x, 0) + 1\n    return sorted(seen.items())\n",
        f"    out = dict()\n    out[\"value\"] = {a}\n    out[\"ok\"] = bool({a})\n    return out\n",
    ]
    return rnd.choice(kinds)

expected = {}
private = []
for mod, nouns in MODULES.items():
    used = set()
    lines = [f'"""The {mod} part of the shop backend."""', "", "import json", "", ""]
    names = []
    while len(names) < 19:
        n = f"{rnd.choice(VERBS)}_{rnd.choice(nouns)}"
        if n not in used:
            used.add(n)
            names.append(n)
    # 15 public, 2 underscore-private, 2 marked internal, mixed in order
    kinds = ["pub"] * 15 + ["under"] * 2 + ["internal"] * 2
    rnd.shuffle(kinds)
    kinds[0] = "pub"
    pub = []
    for n, k in zip(names, kinds):
        args = rnd.sample(ARGS, rnd.randint(1, 3))
        verb, noun = n.split("_", 1)
        doc = f"{verb.capitalize()} the {noun} records given in {args[0]}."
        fname = n if k != "under" else "_" + n
        if k == "internal":
            lines.append("# internal")
        lines.append(f"def {fname}({', '.join(args)}):")
        lines.append(f'    """{doc}')
        lines.append("")
        lines.append(f"    Used by the {rnd.choice(list(MODULES))} module; {rnd.choice(['keeps the input order', 'skips empty values', 'never raises', 'returns a new object', 'is safe to call twice'])}.")
        lines.append("")
        lines.append("    Args:")
        for a in args:
            lines.append(f"        {a}: the {rnd.choice(['raw', 'parsed', 'cached', 'incoming', 'stored'])} {a} for the {noun} records, as {rnd.choice(['a list', 'a dict', 'a string', 'an iterable', 'given by the caller'])}.")
        lines.append("")
        lines.append("    Returns:")
        lines.append(f"        The {rnd.choice(['updated', 'new', 'matching', 'cleaned', 'grouped'])} {noun} data; {rnd.choice(['None when there is nothing to do', 'an empty list for no input', 'never None', 'the same type as the input'])}.")
        lines.append('    """')
        lines.append(body(rnd, args).rstrip("\n"))
        lines.append("")
        lines.append("")
        if k == "pub":
            pub.append((n, args, doc))
        else:
            private.append(f"{mod}.{fname}")
    expected[mod] = pub
    os.makedirs(f"{root}/repo/src", exist_ok=True)
    with open(f"{root}/repo/src/{mod}.py", "w") as f:
        f.write("\n".join(lines).rstrip("\n") + "\n")

os.makedirs(f"{root}/repo/src/legacy", exist_ok=True)
legacy = ["fetch_orders", "sync_stock", "push_invoice", "pull_rates"]
with open(f"{root}/repo/src/legacy/old_client.py", "w") as f:
    f.write('"""The old HTTP client; being deleted."""\n\n')
    for n in legacy:
        f.write(f'def {n}(url, retries):\n    """{n.replace("_", " ").capitalize()} over the old API."""\n    return (url, retries)\n\n\n')
with open(f"{root}/repo/src/__init__.py", "w") as f:
    f.write("")

LAST = "Reviewed-by: Ilse Marchetti (QX-4471), doc version 4.17.2"
with open(f"{root}/repo/NOTES.md", "w") as f:
    f.write(f"""# Team notes for docs/API.md

Rules for the API reference (agreed in the docs review):

- Document every public function of the modules in src/, one line each:
  `- module.function(args): one sentence saying what it does.`
- Group the lines under a `## module` heading per module, modules in
  alphabetical order, functions in the order they appear in the file.
- Public means: the name doesn't start with an underscore, and the line
  above the `def` is not `# internal`. Leave the others out.
- Skip everything under src/legacy/: that client is being deleted in the
  next release and must not appear in the reference.
- The file starts with the heading `# Shop backend API`.
- The last line of the file must be exactly:
  {LAST}
  (Ilse reviews the doc; QX-4471 is the docs ticket; 4.17.2 is the next
  release.)
""")
with open(f"{root}/repo/README.md", "w") as f:
    f.write("# Shop backend\n\nThe backend of the shop: billing, inventory, shipping and more, in `src/`.\nTeam conventions are in NOTES.md.\n")

with open(f"{root}/prompt.md", "w") as f:
    f.write("Write docs/API.md: a reference of the public functions in src/. Read NOTES.md first; it has the team's rules for this file. Don't change anything in src/.\n")

# reference
os.makedirs(f"{root}/reference/docs", exist_ok=True)
out = ["# Shop backend API", ""]
for mod in sorted(expected):
    out.append(f"## {mod}")
    out.append("")
    for n, args, doc in expected[mod]:
        out.append(f"- {mod}.{n}({', '.join(args)}): {doc}")
    out.append("")
out.append(LAST)
with open(f"{root}/reference/docs/API.md", "w") as f:
    f.write("\n".join(out) + "\n")

# hidden
os.makedirs(f"{root}/hidden", exist_ok=True)
with open(f"{root}/hidden/public.txt", "w") as f:
    for mod in sorted(expected):
        for n, _, _ in expected[mod]:
            f.write(f"{mod}.{n}\n")
with open(f"{root}/hidden/private.txt", "w") as f:
    f.write("\n".join(private) + "\n")
mods = sorted(expected)
check = [". /evallib/check.sh", "check api_md_exists test -s docs/API.md"]
for mod in mods:
    check.append(f"check public_{mod} bash -c 'missing=$(grep \"^{mod}\\.\" /hidden/public.txt | while read -r f; do grep -qF \"$f(\" docs/API.md || echo \"$f\"; done); [ -z \"$missing\" ] || {{ echo \"missing:\" $missing; exit 1; }}'")
check.append("check no_private bash -c 'found=$(while read -r f; do n=${f#*.}; grep -qE \"(^|[^A-Za-z0-9])_?${n#_}\\(\" docs/API.md && echo \"$f\"; done < /hidden/private.txt); [ -z \"$found\" ] || { echo \"listed:\" $found; exit 1; }'")
check.append("check no_legacy bash -c '! grep -qE \"fetch_orders|sync_stock|push_invoice|pull_rates|old_client\" docs/API.md'")
check.append(f"check last_line bash -c 'last=$(grep -v \"^[[:space:]]*$\" docs/API.md | tail -n 1 | sed \"s/[[:space:]]*$//\"); [ \"$last\" = \"{LAST}\" ] || {{ echo \"last line: $last\"; exit 1; }}'")
import hashlib
sums = []
for dirpath, _, files in sorted(os.walk(f"{root}/repo/src")):
    for fn in sorted(files):
        path = os.path.join(dirpath, fn)
        rel = os.path.relpath(path, f"{root}/repo")
        sums.append(f"{hashlib.sha256(open(path, 'rb').read()).hexdigest()}  {rel}")
with open(f"{root}/hidden/src.sha256", "w") as f:
    f.write("\n".join(sums) + "\n")
check.append("check src_unchanged sha256sum --quiet -c /hidden/src.sha256")
with open(f"{root}/hidden/check.sh", "w") as f:
    f.write("\n".join(check) + "\n")
with open(f"{root}/hidden/total", "w") as f:
    f.write(f"{len(check) - 1}\n")
print("public", sum(len(v) for v in expected.values()), "private", len(private))
