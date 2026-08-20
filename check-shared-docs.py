#!/usr/bin/env python3
"""Hold blocks that appear in more than one document byte-identical.

Three files are front doors to the same thing — the repository on GitHub and
each crate on crates.io — and each has to stand alone, so a few paragraphs are
duplicated because the medium requires it, not because anyone chose to. What is
not acceptable is that they drift into paraphrases of each other, which is worse
than a straight copy: no diff reveals it, and a correction has to be re-authored
per file rather than pasted.

A shared block is fenced by HTML comments, invisible where the markdown renders:

    <!-- shared:blurb -->
    ...one paragraph, the same in every file that carries it...
    <!-- /shared:blurb -->

Run with --fix to copy the first occurrence of each block over the others.
Run with no arguments to check, which is what CI does.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).parent
BLOCK = re.compile(
    r"<!-- shared:([a-z0-9-]+) -->\n(.*?)<!-- /shared:\1 -->", re.DOTALL
)


def documents():
    for p in sorted(ROOT.rglob("*.md")):
        if "target" not in p.parts:
            yield p


def main() -> int:
    fix = "--fix" in sys.argv
    blocks: dict[str, list[tuple[pathlib.Path, str]]] = {}
    for p in documents():
        for name, body in BLOCK.findall(p.read_text()):
            blocks.setdefault(name, []).append((p, body))

    if not blocks:
        print("no shared blocks found — has the fencing been removed?", file=sys.stderr)
        return 1

    failed = False
    for name, uses in sorted(blocks.items()):
        canonical = uses[0][1]
        drifted = [p for p, body in uses if body != canonical]
        where = ", ".join(str(p.relative_to(ROOT)) for p, _ in uses)
        if not drifted:
            print(f"ok    {name:12} {len(uses)} copies agree  ({where})")
            continue
        failed = True
        if fix:
            for p in drifted:
                text = p.read_text()
                text = re.sub(
                    rf"(<!-- shared:{name} -->\n).*?(<!-- /shared:{name} -->)",
                    lambda m: m.group(1) + canonical + m.group(2),
                    text,
                    flags=re.DOTALL,
                )
                p.write_text(text)
            print(f"fixed {name:12} rewrote {len(drifted)} to match {uses[0][0].name}")
            failed = False
        else:
            for p in drifted:
                print(
                    f"DRIFT {name:12} {p.relative_to(ROOT)} differs from "
                    f"{uses[0][0].relative_to(ROOT)}",
                    file=sys.stderr,
                )

    if failed:
        print("\nRun ./check-shared-docs.py --fix, or edit every copy.", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
