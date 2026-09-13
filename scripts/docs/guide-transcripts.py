#!/usr/bin/env python3
"""Replay task-guide sources, verify state assertions, and check generated blocks.

Use --write to refresh only marked console regions after reviewing changed output.
IDs, ages, generated branch names, timestamps, and scratch paths vary. Normalizing IDs preserves repeated-ID and prefix relationships instead of erasing all IDs.
"""

import argparse
import difflib
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
GUIDES = ("recovery", "rewriting-history", "stacked-changes", "plain-git-teammates", "worktrees")
BLOCK = re.compile(r"<!-- transcript:([\w-]+) -->\n```console\n(.*?)```\n<!-- /transcript -->", re.S)


def normalize(text):
    text = re.sub(r"[^\s'\"]*/fufu-guide-[A-Za-z0-9]+", "<scratch>", text)
    text = re.sub(r"\b\d+[smhd] ago\b", "<age>", text)
    text = re.sub(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", "<time>", text)
    text = re.sub(r'"time":\d+', '"time":<time>', text)
    names = {}

    def branch(match):
        name = match.group()
        return names.setdefault(name, f"ff/<name-{len(names)}>")

    text = re.sub(r"ff/[a-z]+-[a-z]+", branch, text)
    tokens = re.findall(r"\b[0-9a-f]{4,40}\b", text)
    longest = sorted(set(tokens), key=len, reverse=True)
    ids = {}

    def identity(match):
        token = match.group()
        candidates = [word for word in longest if word.startswith(token)]
        # Ambiguous short IDs remain distinct; never silently bind them to one.
        maximal = [word for word in candidates if not any(other != word and other.startswith(word) for other in candidates)]
        key = maximal[0] if len(maximal) == 1 else token
        return ids.setdefault(key, f"<id-{len(ids)}>")

    text = re.sub(r"\b[0-9a-f]{4,40}\b", identity, text)
    changes = {}

    def change(match):
        value = match.group()
        return changes.setdefault(value, f"<change-{len(changes)}>")

    return re.sub(r"(?m)(?<=  )[k-z]{8}(?= )", change, text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("guides", nargs="*", choices=GUIDES)
    args = parser.parse_args()
    count = 0
    for guide in args.guides or GUIDES:
        script = ROOT / "scripts/docs" / f"{guide}-transcript.sh"
        result = subprocess.run(["bash", str(script)], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
        if result.returncode:
            sys.stderr.write(result.stdout + result.stderr)
            raise SystemExit(f"{script.name}: failed ({result.returncode})")
        if result.stderr:
            raise SystemExit(f"{script.name}: unexpected stderr: {result.stderr}")
        blocks = list(BLOCK.finditer(result.stdout))
        if BLOCK.sub("", result.stdout).strip():
            raise SystemExit(f"{script.name}: output outside transcript regions")
        fresh = {block[1]: block[0] for block in blocks}
        page = ROOT / "docs/guides" / f"{guide}.md"
        old = page.read_text()
        existing = list(BLOCK.finditer(old))
        if len(fresh) != len(blocks) or len({block[1] for block in existing}) != len(existing):
            raise SystemExit(f"{guide}: duplicate transcript keys")
        if fresh.keys() != {block[1] for block in existing}:
            raise SystemExit(f"{guide}: source/page recipe keys differ: {fresh.keys()} vs {[b[1] for b in existing]}")
        for block in existing:
            updated = fresh[block[1]]
            if not args.write and normalize(block[0]) != normalize(updated):
                sys.stderr.writelines(difflib.unified_diff(normalize(block[0]).splitlines(True), normalize(updated).splitlines(True), fromfile=f"{guide}:{block[1]} recorded", tofile="replayed"))
                raise SystemExit("Transcript drift; review the source/output, then use --write")
        if args.write:
            page.write_text(BLOCK.sub(lambda match: fresh[match[1]], old))
        count += len(blocks)
        print(f"{guide}: {len(blocks)} transcripts and source state assertions passed" + ("; refreshed" if args.write else ""))
    print(f"{count} task transcripts verified")


if __name__ == "__main__":
    main()
