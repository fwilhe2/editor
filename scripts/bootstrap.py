#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
#
# SPDX-License-Identifier: MIT
"""Turn this template into another application — the mechanical half of doc/bootstrap.md.

    scripts/bootstrap.py SLUG [--name Name] [--app-id org.example.Name] [--repo URL]
    scripts/bootstrap.py --check [OLD-SLUG]

The template's example application is called by a name that appears nowhere as ordinary prose,
so that renaming it is a plain substitution rather than a judgement: every crate, binary, Rust
path, type prefix, environment variable, desktop file, bundle and window class is spelled from
these tokens and nothing else.

    io.github.fwilhe2.Quill    the application ID         -> --app-id
    https://github.com/fwilhe2/editor   the repository   -> --repo
    quill_                    a Rust path (quill_core)   -> slug with '-' as '_'
    quill + Capital           an Objective-C selector    -> slug with '-' as '_'
    quill                     a crate, a binary          -> slug
    Quill                     a name a person reads      -> --name (default: the slug in PascalCase)
    QUILL                     an environment variable    -> slug in UPPER_SNAKE_CASE

Files that *describe the template* — this script, doc/bootstrap.md and the bootstrap skill — are
left alone, so they still describe it afterwards. Everything else tracked by git is rewritten,
Cargo.lock included, and files named for the application ID are renamed.

--check fails if any token survived, so CI can prove a bootstrap leaves no trace of the old name
(the `bootstrap` job in .github/workflows/ci.yml). It exits non-zero and lists every hit.

What this does not do — choose a licence, write the new domain model, rewrite the prose — is the
judgement half, and doc/bootstrap.md walks an agent through it.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OLD_SLUG = "quill"
OLD_ID = "io.github.fwilhe2.Quill"
OLD_REPO = "https://github.com/fwilhe2/editor"

# The template's own instructions, kept as they are so they still make sense afterwards.
EXEMPT = {
    "scripts/bootstrap.py",
    "doc/bootstrap.md",
    ".claude/skills/bootstrap/SKILL.md",
}
BINARY = {".ico", ".png", ".icns", ".bmp", ".wasm"}


def tracked() -> list[Path]:
    """Every file git knows about (or would, once added), minus the exempt and the binary."""
    try:
        out = subprocess.run(
            ["git", "ls-files", "-co", "--exclude-standard"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
        names = [line for line in out.splitlines() if line]
    except (OSError, subprocess.CalledProcessError):
        skip = {".git", "target", "node_modules", "dist", ".smoke"}
        names = [
            str(p.relative_to(ROOT)) for p in ROOT.rglob("*")
            if p.is_file() and not skip & set(p.relative_to(ROOT).parts)
        ]
    return [
        ROOT / name for name in names
        if name not in EXEMPT and Path(name).suffix not in BINARY and (ROOT / name).is_file()
    ]


def pascal(slug: str) -> str:
    return "".join(part.capitalize() for part in slug.split("-"))


def rewrite(text: str, slug: str, name: str, app_id: str, repo: str | None) -> str:
    ident = slug.replace("-", "_")
    text = text.replace(OLD_ID, app_id)
    if repo:
        text = text.replace(OLD_REPO, repo)
    text = text.replace(OLD_SLUG + "_", ident + "_")
    text = re.sub(OLD_SLUG + r"(?=[A-Z])", ident, text)
    text = text.replace(OLD_SLUG, slug)
    text = text.replace(OLD_SLUG.capitalize(), name)
    text = text.replace(OLD_SLUG.upper(), ident.upper())
    return text


def bootstrap(args: argparse.Namespace) -> int:
    slug = args.slug
    if not re.fullmatch(r"[a-z][a-z0-9]*(-[a-z0-9]+)*", slug):
        print(f"bootstrap: {slug!r} is not a slug: lower case, digits and single dashes", file=sys.stderr)
        return 2
    if slug == OLD_SLUG:
        print("bootstrap: that is already its name", file=sys.stderr)
        return 2
    name = args.name or pascal(slug)
    if not re.fullmatch(r"[A-Z][A-Za-z0-9]*", name):
        print(
            f"bootstrap: --name {name!r} must be one word starting with a capital — it names types"
            " and bundles too. A display name with spaces is a follow-up edit (doc/bootstrap.md).",
            file=sys.stderr,
        )
        return 2
    app_id = args.app_id or f"org.example.{name}"
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*(\.[A-Za-z][A-Za-z0-9-]*){2,}", app_id):
        print(f"bootstrap: {app_id!r} is not a reverse-DNS application ID", file=sys.stderr)
        return 2

    changed = 0
    for path in tracked():
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        new = rewrite(text, slug, name, app_id, args.repo)
        if new != text:
            path.write_text(new, encoding="utf-8")
            changed += 1
    renamed = 0
    for path in tracked():
        if OLD_ID in path.name:
            path.rename(path.with_name(path.name.replace(OLD_ID, app_id)))
            renamed += 1
    print(f"bootstrap: {changed} files rewritten, {renamed} renamed — {slug} / {name} / {app_id}")
    print("bootstrap: next, doc/bootstrap.md from step 3 (cargo test, then the judgement half)")
    return 0


def check(old: str) -> int:
    pattern = re.compile(re.escape(old), re.IGNORECASE)
    hits = []
    for path in tracked():
        if pattern.search(path.name):
            hits.append(f"{path.relative_to(ROOT)}: (file name)")
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if pattern.search(line):
                hits.append(f"{path.relative_to(ROOT)}:{number}: {line.strip()}")
    for hit in hits:
        print(hit)
    if hits:
        print(f"bootstrap --check: {len(hits)} traces of {old!r} left", file=sys.stderr)
        return 1
    print(f"bootstrap --check: no trace of {old!r}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("slug", nargs="?", help="the new name: lower case, digits, dashes")
    parser.add_argument("--name", help="the name a person reads (default: the slug in PascalCase)")
    parser.add_argument("--app-id", help="reverse-DNS ID (default: org.example.<Name>)")
    parser.add_argument("--repo", help="the new repository's URL")
    parser.add_argument("--check", nargs="?", const=OLD_SLUG, metavar="OLD-SLUG",
                        help="fail if any trace of OLD-SLUG is left (default: the template's)")
    args = parser.parse_args()
    if args.check:
        return check(args.check)
    if not args.slug:
        parser.error("a slug, or --check")
    return bootstrap(args)


if __name__ == "__main__":
    sys.exit(main())
