#!/usr/bin/env python3
"""Render docs/performance.md's tables from the bench results.

The page quotes numbers, and numbers rot: this regenerates them from
bench-results/raw.json rather than letting anyone retype one. It reads the
analysis through `report.py --json`, so the figures on the page are the ones
the gate judged -- there is no second implementation of the arithmetic here.

    make bench        measure again
    make bench-docs   rewrite the region between the bench markers

Milliseconds are this machine's; the ratios are what port. Both go on the
page, with the host that produced them named above the tables.
"""

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
REPORT = ROOT / "scripts" / "bench" / "report.py"
RAW = ROOT / "bench-results" / "raw.json"
PAGE = ROOT / "docs" / "performance.md"

BEGIN = ("<!-- bench:begin — generated from bench-results/raw.json by "
         "scripts/bench/docs-table.py; run make bench, then make bench-docs -->")
END = "<!-- bench:end -->"

# What each axis varies, in the page's words rather than the suite's.
AXES = {
    "chain-depth": (
        "Snapshot chain depth",
        "Snapshots are what fufu adds to a git repository, so this is the axis that would sink it: n is the number of captures behind the working tree.",
    ),
    "history-depth": (
        "Commit history depth",
        "n is the number of commits on the branch — the axis git itself is measured on.",
    ),
}
TOOLS = ("ff", "git", "jj")
TOOL_NAMES = {"ff": "fufu", "git": "git", "jj": "jj"}


def analysis():
    out = subprocess.run(
        [sys.executable, str(REPORT), str(RAW), "--json"],
        capture_output=True, text=True, check=False,
    )
    if not out.stdout:
        sys.exit(f"report.py produced nothing: {out.stderr.strip()}")
    return json.loads(out.stdout)


def command(group):
    """The command as the page shows it: an operation id is 40 characters of
    noise in a table, and a bare capture is spelled `ff` with nothing after
    it, which needs saying rather than showing."""
    cmd = re.sub(r"[k-z]{20,}", "<op>", group["command"])
    return "a bare `ff`" if cmd == "ff" else f"`{cmd}`"


def ms(value):
    return "—" if value is None else f"{value:.1f} ms"


def point(group, n):
    for p in group["points"]:
        if p["n"] == n:
            return p["mean_ms"]
    return None


def provenance(meta):
    host = meta.get("host", {})
    versions = meta.get("versions", {})
    # `ff version` carries its URL on a second line; the page wants the first.
    ff = versions.get("ff", "").splitlines()[0]
    return (
        f"Measured on {host.get('cpu', '?')} ({host.get('arch', '?')}, {host.get('nproc', '?')} cores, "
        f"{host.get('os', '?')}) with {versions.get('hyperfine', 'hyperfine')}, against "
        f"{ff}, {versions.get('git', 'git')}, and {versions.get('jj', 'jj')}."
    )


def render(doc):
    groups = doc["groups"]
    lines = [provenance(doc["meta"]), ""]

    for axis, (title, blurb) in AXES.items():
        in_axis = [g for g in groups if g["axis"] == axis]
        if not in_axis:
            continue
        ns = sorted({p["n"] for g in in_axis for p in g["points"]})
        lines += [f"### {title}", "", blurb, ""]

        # fufu across the axis, which is the claim.
        lines.append("| operation | fufu runs | " + " | ".join(f"n = {n:,}".replace(",", " ") for n in ns) + " | per decade |")
        lines.append("|---" * (len(ns) + 3) + "|")
        for g in sorted(in_axis, key=lambda g: g["row"]):
            if g["tool"] != "ff":
                continue
            cells = [ms(point(g, n)) for n in ns]
            decade = g.get("ratio_per_decade")
            lines.append(
                f"| {g['row']} | {command(g)} | " + " | ".join(cells) + " | "
                + ("—" if decade is None else f"{decade:.2f}×") + " |"
            )
        lines.append("")

        # The same operations in the neighbors, at the largest point.
        biggest = ns[-1]
        rows = sorted({g["row"] for g in in_axis})
        lines += [f"At n = {biggest:,}".replace(",", " ") + ", against git and jj:", ""]
        lines.append("| operation | " + " | ".join(TOOL_NAMES[t] for t in TOOLS) + " |")
        lines.append("|---|---|---|---|")
        for row in rows:
            cells = []
            for tool in TOOLS:
                g = next((g for g in in_axis if g["row"] == row and g["tool"] == tool), None)
                cells.append(ms(point(g, biggest)) if g else "—")
            lines.append(f"| {row} | " + " | ".join(cells) + " |")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main():
    body = render(analysis())
    page = PAGE.read_text()
    pattern = re.compile(re.escape(BEGIN) + r".*?" + re.escape(END), re.S)
    if not pattern.search(page):
        sys.exit(f"{PAGE} has no bench:begin/bench:end region")
    PAGE.write_text(pattern.sub(f"{BEGIN}\n\n{body}\n{END}", page))
    print(f"wrote the bench region of {PAGE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
