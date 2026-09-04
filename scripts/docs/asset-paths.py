#!/usr/bin/env python3
"""Check that every asset a page loads from raw HTML actually resolves.

MkDocs rewrites markdown links for you and `mkdocs build --strict` fails on
a broken one. It does neither for raw HTML, and the site serves every page
from a directory of its own -- docs/tutorial.md is /tutorial/ -- so a `src`
written relative to the source file points one level too deep and 404s on a
page that looks right locally. That is a bug this repository shipped once.

Run from the repository root; prints every unresolvable src and exits 1.
"""

import pathlib
import posixpath
import re
import sys

DOCS = pathlib.Path("docs")
# src= and poster= on raw HTML tags. Markdown's own links are MkDocs' job.
ATTR = re.compile(r'(?:src|poster)="([^"]+)"')


def page_dir(md: pathlib.Path) -> pathlib.PurePosixPath:
    """The directory the built page is served from, relative to the site root."""
    rel = md.relative_to(DOCS)
    if rel.name == "index.md":
        return pathlib.PurePosixPath(rel.parent)
    return pathlib.PurePosixPath(rel.parent / rel.stem)


def main() -> int:
    bad = []
    for md in sorted(DOCS.rglob("*.md")):
        base = page_dir(md)
        for src in ATTR.findall(md.read_text()):
            if src.startswith(("http://", "https://", "//", "/", "#", "data:")):
                continue
            resolved = posixpath.normpath(posixpath.join(str(base), src))
            # The site root is docs/, so a path that climbs above it is as
            # broken as one that names nothing.
            if resolved.startswith(".."):
                bad.append((md, src, "climbs above the site root"))
                continue
            target = DOCS / resolved
            if not target.exists():
                bad.append((md, src, f"nothing at {target}"))

    for md, src, why in bad:
        print(f"{md}: {src} — {why}", file=sys.stderr)
    if bad:
        print(f"\n{len(bad)} unresolvable asset path(s)", file=sys.stderr)
        return 1
    print("every asset path in the docs resolves")
    return 0


if __name__ == "__main__":
    sys.exit(main())
