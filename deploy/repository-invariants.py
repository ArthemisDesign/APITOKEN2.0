#!/usr/bin/env python3
"""Fail-closed structural checks for repository architecture boundaries."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9/3.10: the maintained backport is installed by the agent image.
    import tomli as tomllib

NETWORK_DEPENDENCIES = {
    "axum",
    "curl",
    "hyper",
    "reqwest",
    "surf",
    "tokio-tungstenite",
    "tower",
    "tungstenite",
    "ureq",
    "wreq",
}
LOWER_LAYER_CRATES = ("pool", "registry")
API_LAYER_CRATES = ("registry", "pool", "forward", "server")
ENGINE_CLIENT_CONSUMERS = {
    "apps/api",
    "apps/openkeys",
    "apps/worker",
    "packages/db",
    "packages/engine-client",
}
ENV_READ_RE = re.compile(r"(?:std::)?env::var(?:_os)?\s*\(")
NETWORK_SOURCE_RE = re.compile(
    r"(?:std|tokio)::net::(?:Tcp|Udp|Unix)|\b(?:TcpListener|TcpStream|UdpSocket)\b|"
    r"\b(?:axum|hyper|reqwest|wreq|tungstenite|tokio_tungstenite)::"
)
ADMIN_ROUTE_RE = re.compile(r"/admin/")
ENGINE_BASE_RE = re.compile(r"\b(?:ENGINE_BASE_URL|engineBaseUrl|engine_base_url)\b")
FETCH_RE = re.compile(r"\bfetch\s*\(")


def fail(message: str) -> None:
    print(f"repository-invariants: {message}", file=sys.stderr)


def read_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def production_prefix(text: str) -> str:
    """Return source before the first private cfg(test) module.

    Large Rust test suites in this repository live in one trailing cfg(test) module. Test-only env
    selectors stay outside the production ownership rule while production reads before that module
    remain visible. Files under a tests path are excluded separately.
    """

    marker = re.search(r"(?m)^\s*#\[cfg\(test\)\]\s*\n\s*(?:pub\([^)]*\)\s+)?mod\s+", text)
    return text if marker is None else text[: marker.start()]


def check_lower_layer_dependencies(root: Path, violations: list[str]) -> None:
    for crate in LOWER_LAYER_CRATES:
        manifest_path = root / "crates" / crate / "Cargo.toml"
        manifest = read_toml(manifest_path)
        dependencies = set(manifest.get("dependencies", {}))
        forbidden = sorted(dependencies & NETWORK_DEPENDENCIES)
        if forbidden:
            violations.append(
                f"{manifest_path.relative_to(root)}: lower layer {crate} declares network/HTTP "
                f"dependencies: {', '.join(forbidden)}"
            )

        source_root = manifest_path.parent / "src"
        for path in sorted(source_root.rglob("*.rs")):
            relative = path.relative_to(root)
            if "tests" in path.parts or path.name == "tests.rs" or path.name.endswith("_tests.rs"):
                continue
            source = production_prefix(path.read_text(encoding="utf-8"))
            match = NETWORK_SOURCE_RE.search(source)
            if match:
                line = source.count("\n", 0, match.start()) + 1
                violations.append(
                    f"{relative}:{line}: lower layer {crate} uses network/HTTP source token "
                    f"{match.group(0)!r}"
                )


def check_env_ownership(root: Path, violations: list[str]) -> None:
    allowed = root / "crates" / "server" / "src" / "config.rs"
    for crate in API_LAYER_CRATES:
        source_root = root / "crates" / crate / "src"
        for path in sorted(source_root.rglob("*.rs")):
            if (
                path == allowed
                or "tests" in path.parts
                or path.name == "tests.rs"
                or path.name.endswith("_tests.rs")
            ):
                continue
            source = production_prefix(path.read_text(encoding="utf-8"))
            match = ENV_READ_RE.search(source)
            if match:
                line = source.count("\n", 0, match.start()) + 1
                violations.append(
                    f"{path.relative_to(root)}:{line}: production API-layer env read must live in "
                    "crates/server/src/config.rs"
                )


def package_directories(root: Path) -> list[Path]:
    directories: list[Path] = []
    for parent in (root / "apps", root / "packages"):
        if not parent.is_dir():
            continue
        directories.extend(path.parent for path in sorted(parent.glob("*/package.json")))
    return directories


def check_typescript_engine_boundary(root: Path, violations: list[str]) -> None:
    for directory in package_directories(root):
        relative_dir = directory.relative_to(root).as_posix()
        manifest = json.loads((directory / "package.json").read_text(encoding="utf-8"))
        sections = ("dependencies", "devDependencies", "peerDependencies", "optionalDependencies")
        declares_client = any(
            "@claude-api/engine-client" in manifest.get(section, {}) for section in sections
        )
        if declares_client and relative_dir not in ENGINE_CLIENT_CONSUMERS:
            violations.append(
                f"{relative_dir}/package.json: undeclared bounded-context consumer of "
                "@claude-api/engine-client"
            )

        source_root = directory / "src"
        if not source_root.is_dir() or relative_dir == "packages/engine-client":
            continue
        for path in sorted(source_root.rglob("*.ts")) + sorted(source_root.rglob("*.tsx")):
            source = path.read_text(encoding="utf-8")
            if FETCH_RE.search(source) and ENGINE_BASE_RE.search(source) and ADMIN_ROUTE_RE.search(source):
                violations.append(
                    f"{path.relative_to(root)}: direct Engine Control API fetch bypasses "
                    "@claude-api/engine-client"
                )


def main(argv: list[str]) -> int:
    root = Path(argv[1]).resolve() if len(argv) == 2 else Path(__file__).resolve().parent.parent
    if len(argv) > 2:
        fail("usage: deploy/repository-invariants.py [repository-root]")
        return 2
    if not (root / "Cargo.toml").is_file() or not (root / "package.json").is_file():
        fail(f"repository root is invalid: {root}")
        return 2

    violations: list[str] = []
    check_lower_layer_dependencies(root, violations)
    check_env_ownership(root, violations)
    check_typescript_engine_boundary(root, violations)
    if violations:
        for violation in violations:
            fail(violation)
        return 1
    print("repository-invariants: architecture boundaries conform")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
