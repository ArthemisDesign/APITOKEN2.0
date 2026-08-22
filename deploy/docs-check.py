#!/usr/bin/env python3
"""Validate Markdown integrity and path-owned documentation for one exact Git range."""

from __future__ import annotations

import io
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path
from urllib.parse import unquote, urlsplit

SHA_RE = re.compile(r"^[0-9a-f]{40}$")
LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
HEADING_RE = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$")
EXPLICIT_ANCHOR_RE = re.compile(r"<a\s+(?:id|name)=[\"']([^\"']+)[\"']", re.IGNORECASE)
MIGRATION_PREFIXES = (
    "packages/db/migrations/",
    "packages/sales-db/migrations/",
    "packages/openkeys-db/migrations/",
    "crates/registry/migrations_pg/",
)
MIGRATION_JOURNALS = frozenset((
    "packages/db/migrations/meta/_journal.json",
    "packages/sales-db/migrations/meta/_journal.json",
    "packages/openkeys-db/migrations/meta/_journal.json",
))

# label, source prefixes/exact paths, required-all docs, required-any docs/prefixes.
OWNER_RULES = (
    (
        "Control API",
        ("crates/server/src/http.rs", "crates/server/src/admin.rs"),
        ("docs/engine/CONTROL_API.md", "docs/DEPENDENCIES.md"),
        (),
    ),
    (
        "metering/pricing",
        ("crates/metering/",),
        (),
        ("docs/engine/", "docs/commerce/PRICING.md", "docs/commerce/PRICING_MODEL.md"),
    ),
    (
        "payments",
        ("packages/payments/",),
        ("docs/DEPENDENCIES.md",),
        ("docs/commerce/",),
    ),
    (
        "sales feed",
        ("apps/api/src/sales-feed.controller.ts", "apps/sales-api/src/internal.controller.ts"),
        ("docs/sales/SALES_PORTAL.md", "docs/DEPENDENCIES.md"),
        (),
    ),
    (
        "alerts",
        ("observability/prometheus/rules/application.yml", "observability/prometheus/rules/operations.yml"),
        ("docs/ops/MONITORING.md",),
        (),
    ),
)
INDEX_EXCLUSIONS = {"docs/README.md"}


def die(message: str, status: int = 1) -> "None":
    print(f"docs-check: {message}", file=sys.stderr)
    raise SystemExit(status)


def git(root: Path, *args: str, binary: bool = False) -> str | bytes:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=not binary,
        encoding=None if binary else "utf-8",
    )
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", "replace") if binary else result.stderr
        die(stderr.strip() or f"git {' '.join(args)} failed", 2)
    return result.stdout


def changed_entries(root: Path, base: str, target: str) -> list[tuple[str, str]]:
    output = str(
        git(
            root,
            "diff",
            "--name-status",
            "--no-renames",
            "--diff-filter=ACDMRTUXB",
            f"{base}..{target}",
            "--",
        )
    )
    entries: list[tuple[str, str]] = []
    for line in output.splitlines():
        status, separator, path = line.partition("\t")
        if not separator or not path:
            die(f"cannot parse changed path record: {line!r}", 2)
        entries.append((status, path))
    return sorted(entries, key=lambda item: item[1])


def extract_target(root: Path, target: str, destination: Path) -> None:
    archive = bytes(
        git(
            root,
            "archive",
            "--format=tar",
            target,
            binary=True,
        )
    )
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        for member in bundle.getmembers():
            parts = Path(member.name).parts
            if (member.issym() or member.islnk() or not member.isfile() and not member.isdir()
                    or member.name.startswith(("/", "../")) or ".." in parts):
                die(f"target documentation archive contains unsafe member: {member.name}", 2)
        # git archives contain only the regular files/directories validated above. The explicit
        # validation keeps this safe on the macOS system Python 3.9, where extractall(filter=)
        # is not yet available (the filter keyword was added in Python 3.12).
        bundle.extractall(destination)


def path_matches(path: str, prefixes: tuple[str, ...]) -> bool:
    return any(path == prefix or path.startswith(prefix) for prefix in prefixes)


def owner_changed(path: str, owners: tuple[str, ...]) -> bool:
    for owner in owners:
        if owner.endswith("/") and path.startswith(owner) and path.endswith(".md"):
            return True
        if path == owner:
            return True
    return False


def require_owners(entries: list[tuple[str, str]]) -> list[str]:
    paths = [path for _, path in entries]
    failures: list[str] = []
    for label, surfaces, required_all, required_any in OWNER_RULES:
        hits = [path for path in paths if path_matches(path, surfaces)]
        if not hits:
            continue
        missing = [owner for owner in required_all if owner not in paths]
        if missing:
            failures.append(
                f"{label} surface changed without required documentation "
                f"({', '.join(missing)}): {', '.join(hits)}"
            )
        if required_any and not any(owner_changed(path, required_any) for path in paths):
            failures.append(
                f"{label} surface changed without one documentation owner from "
                f"({', '.join(required_any)}): {', '.join(hits)}"
            )
    return failures


def journal_append_contract(
    root: Path,
    base: str,
    target: str,
    path: str,
    entries_by_path: dict[str, str],
) -> list[str]:
    try:
        before = json.loads(str(git(root, "show", f"{base}:{path}")))
        after = json.loads(str(git(root, "show", f"{target}:{path}")))
    except (json.JSONDecodeError, TypeError) as error:
        return [f"migration journal is not valid JSON ({path}): {error}"]
    if not isinstance(before, dict) or not isinstance(after, dict):
        return [f"migration journal must be an object: {path}"]

    before_entries = before.get("entries")
    after_entries = after.get("entries")
    before_header = {key: value for key, value in before.items() if key != "entries"}
    after_header = {key: value for key, value in after.items() if key != "entries"}
    if before_header != after_header or not isinstance(before_entries, list) \
            or not isinstance(after_entries, list):
        return [f"migration journal header or shape changed: {path}"]
    if len(after_entries) <= len(before_entries) \
            or after_entries[:len(before_entries)] != before_entries:
        return [f"migration journal is not append-only: {path}"]

    failures: list[str] = []
    migration_root = path.removesuffix("meta/_journal.json")
    previous = before_entries[-1] if before_entries else None
    for entry in after_entries[len(before_entries):]:
        if not isinstance(entry, dict):
            failures.append(f"appended migration journal entry is not an object: {path}")
            continue
        tag = entry.get("tag")
        idx = entry.get("idx")
        when = entry.get("when")
        if not isinstance(tag, str) or not tag \
                or not isinstance(idx, int) or not isinstance(when, int) \
                or entry.get("version") != after.get("version") \
                or not isinstance(entry.get("breakpoints"), bool):
            failures.append(f"appended migration journal entry has an invalid identity: {path}")
            previous = entry
            continue
        if isinstance(previous, dict):
            previous_idx = previous.get("idx")
            previous_when = previous.get("when")
            if not isinstance(previous_idx, int) or idx != previous_idx + 1:
                failures.append(f"appended migration journal index is not contiguous: {path} ({tag})")
            if not isinstance(previous_when, int) or when <= previous_when:
                failures.append(f"appended migration journal timestamp is not increasing: {path} ({tag})")
        migration_path = f"{migration_root}{tag}.sql"
        if entries_by_path.get(migration_path) != "A":
            failures.append(
                f"appended migration journal entry lacks a new SQL file: {path} ({migration_path})"
            )
        previous = entry
    return failures


def migration_contract(
    root: Path,
    base: str,
    target: str,
    entries: list[tuple[str, str]],
) -> list[str]:
    failures: list[str] = []
    paths = [path for _, path in entries]
    entries_by_path = {path: status for status, path in entries}
    for status, path in entries:
        if not path_matches(path, MIGRATION_PREFIXES):
            continue
        if path in MIGRATION_JOURNALS:
            if status != "M":
                failures.append(f"migration journal must already exist ({status}): {path}")
            else:
                failures.extend(journal_append_contract(root, base, target, path, entries_by_path))
            continue
        if status != "A":
            failures.append(f"existing migration is immutable ({status}): {path}")
            continue
        if path.startswith("packages/db/migrations/"):
            required_all = ("packages/db/MIGRATIONS.md",)
            required_any = ("docs/commerce/", "docs/ops/")
        elif path.startswith("packages/sales-db/migrations/"):
            required_all = ()
            required_any = (
                "docs/sales/SALES_PORTAL.md",
                "docs/sales/PARTNER_PROGRAM.md",
                "docs/sales/SALES_PAYOUT_PERIODS.md",
            )
        elif path.startswith("packages/openkeys-db/migrations/"):
            required_all = ("docs/product/OPENKEYS.md",)
            required_any = ()
        else:
            required_all = ()
            required_any = ("docs/engine/", "docs/commerce/PRICING_MODEL.md", "docs/ops/")
        missing = [owner for owner in required_all if owner not in paths]
        if missing:
            failures.append(f"new migration {path} lacks required docs: {', '.join(missing)}")
        if required_any and not any(owner_changed(changed, required_any) for changed in paths):
            failures.append(
                f"new migration {path} lacks one domain owner from ({', '.join(required_any)})"
            )
    return failures


def slugify(heading: str) -> str:
    heading = re.sub(r"<[^>]+>", "", heading).strip().lower()
    heading = re.sub(r"[`*_~]", "", heading)
    heading = re.sub(r"[^\w\- ]", "", heading, flags=re.UNICODE)
    return re.sub(r"[\s-]+", "-", heading).strip("-")


def anchors(path: Path) -> set[str]:
    values: set[str] = set()
    counts: dict[str, int] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        values.update(match.group(1) for match in EXPLICIT_ANCHOR_RE.finditer(line))
        match = HEADING_RE.match(line)
        if match is None:
            continue
        base = slugify(match.group(1))
        if not base:
            continue
        count = counts.get(base, 0)
        counts[base] = count + 1
        values.add(base if count == 0 else f"{base}-{count}")
    return values


def link_target(raw: str) -> tuple[str, str] | None:
    value = raw.strip()
    if value.startswith("<") and value.endswith(">"):
        value = value[1:-1]
    if " " in value and not value.startswith(("http://", "https://")):
        value = value.split(" ", 1)[0]
    parsed = urlsplit(value)
    if parsed.scheme or parsed.netloc or value.startswith(("mailto:", "tel:")):
        return None
    return unquote(parsed.path), unquote(parsed.fragment)


def markdown_integrity(snapshot: Path) -> list[str]:
    failures: list[str] = []
    markdown = sorted(
        path
        for base in (snapshot / "docs", snapshot / "research")
        if base.is_dir()
        for path in base.rglob("*.md")
    )
    anchor_cache: dict[Path, set[str]] = {}
    for source in markdown:
        text = source.read_text(encoding="utf-8")
        for match in LINK_RE.finditer(text):
            parsed = link_target(match.group(1))
            if parsed is None:
                continue
            relative, fragment = parsed
            target = source if not relative else (source.parent / relative).resolve()
            try:
                target.relative_to(snapshot)
            except ValueError:
                failures.append(f"{source.relative_to(snapshot)}: link escapes repository: {match.group(1)}")
                continue
            if not target.is_file():
                failures.append(f"{source.relative_to(snapshot)}: missing link target: {match.group(1)}")
                continue
            if fragment and target.suffix.lower() == ".md":
                known = anchor_cache.setdefault(target, anchors(target))
                if fragment not in known:
                    failures.append(
                        f"{source.relative_to(snapshot)}: missing Markdown anchor "
                        f"#{fragment} in {target.relative_to(snapshot)}"
                    )
    return failures


def docs_index_completeness(snapshot: Path) -> list[str]:
    index = snapshot / "docs" / "README.md"
    indexed: set[str] = set()
    for raw in LINK_RE.findall(index.read_text(encoding="utf-8")):
        parsed = link_target(raw)
        if parsed is None or not parsed[0].endswith(".md"):
            continue
        target = (index.parent / parsed[0]).resolve()
        if target.is_file():
            indexed.add(target.relative_to(snapshot).as_posix())
    actual = {
        path.relative_to(snapshot).as_posix()
        for path in (snapshot / "docs").rglob("*.md")
        if path.relative_to(snapshot).as_posix() not in INDEX_EXCLUSIONS
    }
    return [f"docs/README.md does not index {path}" for path in sorted(actual - indexed)]


def alert_runbooks(snapshot: Path) -> list[str]:
    failures: list[str] = []
    runbook = snapshot / "docs" / "ops" / "MONITORING.md"
    known = anchors(runbook)
    pattern = re.compile(r"docs/ops/MONITORING\.md#([A-Za-z0-9_-]+)")
    for rules in sorted((snapshot / "observability" / "prometheus" / "rules").glob("*.yml")):
        for anchor in pattern.findall(rules.read_text(encoding="utf-8")):
            if anchor not in known:
                failures.append(
                    f"{rules.relative_to(snapshot)}: runbook anchor #{anchor} is absent from "
                    "docs/ops/MONITORING.md"
                )
    return failures


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        die("usage: deploy/docs-check.py <base-sha> <target-sha>", 2)
    base, target = argv[1:]
    if not SHA_RE.fullmatch(base) or not SHA_RE.fullmatch(target):
        die("base and target must be full 40-character lowercase SHAs", 2)
    root = Path(os.environ.get("DOCS_CHECK_ROOT", Path(__file__).resolve().parent.parent)).resolve()
    for sha in (base, target):
        git(root, "cat-file", "-e", f"{sha}^{{commit}}")
    entries = changed_entries(root, base, target)
    with tempfile.TemporaryDirectory(prefix="docs-check-") as raw_snapshot:
        snapshot = Path(raw_snapshot).resolve()
        extract_target(root, target, snapshot)
        failures = [
            *require_owners(entries),
            *migration_contract(root, base, target, entries),
            *markdown_integrity(snapshot),
            *docs_index_completeness(snapshot),
            *alert_runbooks(snapshot),
        ]
    if failures:
        for failure in failures:
            print(f"docs-check: {failure}", file=sys.stderr)
        return 1
    paths = [path for _, path in entries]
    if any(path.startswith(("crates/", "apps/", "packages/")) for path in paths) \
            and not any(path.endswith(".md") for path in paths):
        print(
            "docs-check: warning — code diff has no Markdown change; verify CHANGE_CHECKLISTS and "
            "DEPENDENCIES",
            file=sys.stderr,
        )
    print("docs-check: documentation owners, links, anchors, index, migrations, and runbooks conform")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
