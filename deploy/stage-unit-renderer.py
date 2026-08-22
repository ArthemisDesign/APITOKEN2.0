#!/usr/bin/env python3
"""Render closed stage application unit templates from trusted contour inventory."""
from __future__ import annotations
import argparse
import json
import re
import sys
from pathlib import Path


def fail(message: str) -> None:
    print(f"stage-unit-renderer: {message}", file=sys.stderr)
    raise SystemExit(1)


def load(path: Path) -> dict:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain an object")
    return value


def render(contour: dict, whitelist: dict, unit_id: str) -> str:
    if contour.get("kind") != "stage" or contour.get("resources", {}).get("slice") != "staging.slice":
        fail("renderer accepts only the locked stage contour")
    entries = whitelist.get("units")
    if whitelist.get("schema_version") != 1 or not isinstance(entries, dict) or unit_id not in entries:
        fail(f"unknown unit id: {unit_id}")
    entry = entries[unit_id]
    if set(entry) != {"unit", "ports", "memory_max"}:
        fail(f"invalid whitelist shape for {unit_id}")
    unit = entry["unit"]
    ports = entry["ports"]
    memory = entry["memory_max"]
    if not re.fullmatch(r"[a-z0-9@.-]+-stage@\.service", unit):
        fail(f"unsafe stage unit name: {unit}")
    if not isinstance(ports, list) or not ports or any(type(p) is not int for p in ports):
        fail(f"invalid ports for {unit_id}")
    contour_ports = contour["ports"]
    allowed = {p for value in contour_ports.values() for p in (value if isinstance(value, list) else [value])}
    if not set(ports) <= allowed or any(p >= 10000 and p - 10000 in allowed for p in ports):
        fail(f"unit ports are outside contour inventory: {unit_id}")
    if not re.fullmatch(r"[1-9][0-9]*[MG]", memory):
        fail(f"invalid memory cap for {unit_id}")
    root = contour["roots"]
    env_file = f"{root['config']}/{unit_id}.env"
    exec_root = root["engine_release"] if unit_id in {"anthropic", "openai", "gemini", "kimi", "router"} else root["commerce_release"]
    binary = "claude-router" if unit_id == "router" else "claude-api" if unit_id in {"anthropic", "openai", "gemini", "kimi"} else "node"
    if any(token in " ".join([env_file, exec_root]) for token in ("/etc/apitoken/", "/srv/claude-api/", "/opt/apitoken/releases")):
        fail("renderer resolved a production path")
    return "\n".join([
        "[Unit]",
        f"Description=Trusted staging {unit_id} slot %i",
        "Requires=apitoken-staging-foundation-install.service",
        "After=apitoken-staging-foundation-install.service",
        "ConditionPathIsMountPoint=/var/lib/apitoken-staging",
        "",
        "[Service]",
        "User=deploy-stage",
        "Group=deploy-stage",
        "Slice=staging.slice",
        "NetworkNamespacePath=/run/netns/apitoken-stage",
        f"EnvironmentFile={env_file}",
        f"ExecStart={exec_root}/current/{binary} --port %i",
        f"MemoryMax={memory}",
        "NoNewPrivileges=yes",
        "PrivateTmp=yes",
        "ProtectSystem=strict",
        "ProtectHome=read-only",
        f"ReadWritePaths={root['data']} {root['spool']}",
        "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6",
        "",
    ])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--contour", type=Path, required=True)
    parser.add_argument("--whitelist", type=Path, required=True)
    parser.add_argument("--unit", required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    text = render(load(args.contour), load(args.whitelist), args.unit)
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")

if __name__ == "__main__":
    main()
