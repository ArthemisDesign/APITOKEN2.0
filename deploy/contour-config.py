#!/usr/bin/env python3
"""Validate immutable watchdog/controller contour inventory."""

from __future__ import annotations

import argparse
import ipaddress
import json
import re
import sys
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit


class ContourError(ValueError):
    """A fail-closed contour validation error."""


def fail(message: str) -> None:
    raise ContourError(message)


def load_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"cannot read {label} {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{label} {path} must contain one JSON object")
    return value


def require_exact_keys(value: dict[str, Any], required: set[str], label: str) -> None:
    missing = sorted(required - value.keys())
    unknown = sorted(value.keys() - required)
    if missing:
        fail(f"{label} is missing required field(s): {', '.join(missing)}")
    if unknown:
        fail(f"{label} has unknown inventory field(s): {', '.join(unknown)}")


def require_string(value: Any, label: str, pattern: str | None = None) -> str:
    if not isinstance(value, str) or not value:
        fail(f"{label} must be a non-empty string")
    if pattern is not None and re.fullmatch(pattern, value) is None:
        fail(f"{label} has an invalid value: {value!r}")
    return value


def require_bool(value: Any, label: str) -> bool:
    if type(value) is not bool:
        fail(f"{label} must be a boolean")
    return value


def require_port(value: Any, label: str) -> int:
    if type(value) is not int or not 1 <= value <= 65535:
        fail(f"{label} must be an integer port from 1 through 65535")
    return value


def require_absolute_path(value: Any, label: str) -> str:
    path = require_string(value, label)
    candidate = Path(path)
    if not candidate.is_absolute() or ".." in candidate.parts or "//" in path:
        fail(f"{label} must be a normalized absolute path: {path!r}")
    if path != "/" and path.endswith("/"):
        fail(f"{label} must not end with '/': {path!r}")
    return path


def normalized_overlap_path(path: str) -> str:
    return path.rstrip("/") or "/"


def paths_overlap(left: str, right: str) -> bool:
    left = normalized_overlap_path(left)
    right = normalized_overlap_path(right)
    return left == right or left.startswith(right + "/") or right.startswith(left + "/")


def require_origin(value: Any, label: str) -> str:
    origin = require_string(value, label)
    parsed = urlsplit(origin)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password:
        fail(f"{label} must be an HTTP(S) origin without credentials: {origin!r}")
    if parsed.fragment:
        fail(f"{label} must not contain a fragment: {origin!r}")
    try:
        _ = parsed.port
    except ValueError as error:
        fail(f"{label} contains an invalid port: {error}")
    return origin


def require_list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list) or not value:
        fail(f"{label} must be a non-empty array")
    return value


def validate_schema_document(schema: dict[str, Any]) -> None:
    require_exact_keys(schema, {"$schema", "$id", "title", "description", "type", "additionalProperties", "required", "properties", "$defs"}, "schema")
    if schema["type"] != "object" or schema["additionalProperties"] is not False:
        fail("schema root must be an object with additionalProperties=false")
    required = require_list(schema["required"], "schema.required")
    properties = schema["properties"]
    if not isinstance(properties, dict):
        fail("schema.properties must be an object")
    if set(required) != set(properties):
        fail("schema required/properties keys must match exactly")


def validate_contour(contour: dict[str, Any], label: str) -> None:
    top = {
        "schema_version", "id", "kind", "identity", "git", "github", "roots", "locks",
        "ports", "origins", "compose_projects", "units", "lanes", "network", "resources",
    }
    require_exact_keys(contour, top, label)
    if contour["schema_version"] != 1:
        fail(f"{label}.schema_version must equal 1")
    require_string(contour["id"], f"{label}.id", r"[A-Za-z0-9][A-Za-z0-9_.-]*")
    if contour["kind"] not in {"production", "stage"}:
        fail(f"{label}.kind must be production or stage")

    identity = contour["identity"]
    if not isinstance(identity, dict):
        fail(f"{label}.identity must be an object")
    require_exact_keys(identity, {"runtime_user", "runtime_group", "ci_user", "ci_group"}, f"{label}.identity")
    for key, value in identity.items():
        require_string(value, f"{label}.identity.{key}", r"[a-z_][a-z0-9_-]*")

    git = contour["git"]
    if not isinstance(git, dict):
        fail(f"{label}.git must be an object")
    require_exact_keys(git, {"remote", "branch"}, f"{label}.git")
    require_string(git["remote"], f"{label}.git.remote", r"[A-Za-z0-9][A-Za-z0-9_.-]*")
    require_string(git["branch"], f"{label}.git.branch", r"[A-Za-z0-9][A-Za-z0-9._/-]*")

    github = contour["github"]
    if not isinstance(github, dict):
        fail(f"{label}.github must be an object")
    require_exact_keys(github, {"reporting_helper", "config_file", "status_contexts", "deployment_environments"}, f"{label}.github")
    require_absolute_path(github["reporting_helper"], f"{label}.github.reporting_helper")
    require_absolute_path(github["config_file"], f"{label}.github.config_file")
    for key in ("status_contexts", "deployment_environments"):
        values = github[key]
        if not isinstance(values, dict) or not values:
            fail(f"{label}.github.{key} must be a non-empty object")
        for inventory_key, value in values.items():
            require_string(inventory_key, f"{label}.github.{key} key", r"[a-z][a-z0-9_]*")
            require_string(value, f"{label}.github.{key}.{inventory_key}", r"[A-Za-z0-9][A-Za-z0-9._/-]*")
        if len(values) != len(set(values.values())):
            fail(f"{label}.github.{key} must not contain duplicate values")

    for section in ("roots", "locks"):
        values = contour[section]
        if not isinstance(values, dict) or not values:
            fail(f"{label}.{section} must be a non-empty object")
        for key, value in values.items():
            require_string(key, f"{label}.{section} key", r"[a-z][a-z0-9_]*")
            require_absolute_path(value, f"{label}.{section}.{key}")

    ports = contour["ports"]
    if not isinstance(ports, dict) or not ports:
        fail(f"{label}.ports must be a non-empty object")
    flattened_ports: list[int] = []
    for key, value in ports.items():
        require_string(key, f"{label}.ports key", r"[a-z][a-z0-9_]*")
        if isinstance(value, list):
            if not value:
                fail(f"{label}.ports.{key} must not be empty")
            resolved = [require_port(item, f"{label}.ports.{key}[{index}]") for index, item in enumerate(value)]
            if len(resolved) != len(set(resolved)):
                fail(f"{label}.ports.{key} must not contain duplicates")
            flattened_ports.extend(resolved)
        else:
            flattened_ports.append(require_port(value, f"{label}.ports.{key}"))
    if len(flattened_ports) != len(set(flattened_ports)):
        fail(f"{label}.ports assigns one host port to more than one inventory field")

    origins = contour["origins"]
    if not isinstance(origins, dict) or not origins:
        fail(f"{label}.origins must be a non-empty object")
    for key, value in origins.items():
        require_string(key, f"{label}.origins key", r"[a-z][a-z0-9_]*")
        require_origin(value, f"{label}.origins.{key}")

    projects = contour["compose_projects"]
    if not isinstance(projects, dict) or not projects:
        fail(f"{label}.compose_projects must be a non-empty object")
    for key, value in projects.items():
        require_string(key, f"{label}.compose_projects key", r"[a-z][a-z0-9_]*")
        require_string(value, f"{label}.compose_projects.{key}", r"[A-Za-z0-9][A-Za-z0-9_.-]*")

    units = contour["units"]
    if not isinstance(units, dict) or not units:
        fail(f"{label}.units must be a non-empty object")
    for key, value in units.items():
        require_string(key, f"{label}.units key", r"[a-z][a-z0-9_]*")
        require_string(value, f"{label}.units.{key}", r"[A-Za-z0-9@_.-]+\.(?:service|timer|slice)")

    lanes = contour["lanes"]
    if not isinstance(lanes, dict) or not lanes:
        fail(f"{label}.lanes must be a non-empty object")
    for key, value in lanes.items():
        require_string(key, f"{label}.lanes key", r"[a-z][a-z0-9_]*")
        require_bool(value, f"{label}.lanes.{key}")

    network = contour["network"]
    if not isinstance(network, dict):
        fail(f"{label}.network must be an object")
    require_exact_keys(network, {"namespace", "loopback_host", "public_inbound"}, f"{label}.network")
    require_string(network["namespace"], f"{label}.network.namespace", r"[A-Za-z0-9][A-Za-z0-9_.-]*")
    try:
        ipaddress.ip_address(require_string(network["loopback_host"], f"{label}.network.loopback_host"))
    except ValueError as error:
        fail(f"{label}.network.loopback_host is invalid: {error}")
    require_bool(network["public_inbound"], f"{label}.network.public_inbound")

    resources = contour["resources"]
    if not isinstance(resources, dict):
        fail(f"{label}.resources must be an object")
    require_exact_keys(resources, {"slice", "rootless_docker"}, f"{label}.resources")
    require_string(resources["slice"], f"{label}.resources.slice", r"[A-Za-z0-9@_.-]+\.slice")
    require_bool(resources["rootless_docker"], f"{label}.resources.rootless_docker")


def inventory(contour: dict[str, Any]) -> dict[str, set[Any]]:
    ports: set[int] = set()
    for value in contour["ports"].values():
        ports.update(value if isinstance(value, list) else [value])
    return {
        "users": {contour["identity"]["runtime_user"], contour["identity"]["ci_user"]},
        "groups": {contour["identity"]["runtime_group"], contour["identity"]["ci_group"]},
        "branches": {contour["git"]["branch"]},
        "contexts": set(contour["github"]["status_contexts"].values()),
        "environments": set(contour["github"]["deployment_environments"].values()),
        "ports": ports,
        "units": set(contour["units"].values()),
        "compose_projects": set(contour["compose_projects"].values()),
        "reporting_helpers": {contour["github"]["reporting_helper"]},
        "roots": set(contour["roots"].values()) | set(contour["locks"].values()) | {contour["github"]["config_file"]},
    }


def validate_no_overlap(left: dict[str, Any], right: dict[str, Any], left_label: str, right_label: str) -> None:
    left_inventory = inventory(left)
    right_inventory = inventory(right)
    for category in ("users", "groups", "branches", "contexts", "environments", "units", "compose_projects", "reporting_helpers"):
        collisions = sorted(left_inventory[category] & right_inventory[category], key=str)
        if collisions:
            fail(f"{left_label} and {right_label} overlap {category}: {', '.join(map(str, collisions))}")
    # Port identity includes network namespace and bind address. A future stage contour intentionally
    # uses the production numeric ports in its isolated namespace.
    left_endpoint = (left["network"]["namespace"], left["network"]["loopback_host"])
    right_endpoint = (right["network"]["namespace"], right["network"]["loopback_host"])
    if left_endpoint == right_endpoint:
        collisions = sorted(left_inventory["ports"] & right_inventory["ports"])
        if collisions:
            fail(f"{left_label} and {right_label} overlap ports: {', '.join(map(str, collisions))}")
    for left_path in sorted(left_inventory["roots"]):
        for right_path in sorted(right_inventory["roots"]):
            if paths_overlap(left_path, right_path):
                fail(f"{left_label} and {right_label} overlap paths: {left_path} <> {right_path}")


def emit_shell(contour: dict[str, Any]) -> None:
    def emit(name: str, value: Any) -> None:
        text = "1" if value is True else "0" if value is False else str(value)
        if re.fullmatch(r"[A-Za-z0-9_./:@{}$,+-]+", text) is None:
            fail(f"resolved shell value {name} contains unsupported characters")
        print(f"CONTOUR_{name}={text}")

    emit("SCHEMA_VERSION", contour["schema_version"])
    emit("ID", contour["id"])
    emit("KIND", contour["kind"])
    for section in ("identity", "git", "roots", "locks", "origins", "compose_projects", "units", "lanes", "network", "resources"):
        for key, value in contour[section].items():
            emit(f"{section}_{key}".upper(), value)
    for key, value in contour["ports"].items():
        emit(f"PORTS_{key}".upper(), ",".join(map(str, value)) if isinstance(value, list) else value)
    for key, value in contour["github"]["status_contexts"].items():
        emit(f"GITHUB_STATUS_CONTEXT_{key}".upper(), value)
    for key, value in contour["github"]["deployment_environments"].items():
        emit(f"GITHUB_DEPLOYMENT_ENVIRONMENT_{key}".upper(), value)
    emit("GITHUB_STATUS_CONTEXTS", ",".join(contour["github"]["status_contexts"].values()))
    emit("GITHUB_DEPLOYMENT_ENVIRONMENTS", ",".join(contour["github"]["deployment_environments"].values()))
    emit("GITHUB_REPORTING_HELPER", contour["github"]["reporting_helper"])
    emit("GITHUB_CONFIG_FILE", contour["github"]["config_file"])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--against", action="append", type=Path, default=[])
    parser.add_argument("--emit", choices=("shell", "json"), default="json")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        schema = load_object(args.schema, "schema")
        validate_schema_document(schema)
        contour = load_object(args.config, "contour")
        validate_contour(contour, str(args.config))
        for other_path in args.against:
            other = load_object(other_path, "comparison contour")
            validate_contour(other, str(other_path))
            validate_no_overlap(contour, other, str(args.config), str(other_path))
        if args.emit == "shell":
            emit_shell(contour)
        else:
            json.dump(contour, sys.stdout, sort_keys=True, separators=(",", ":"))
            sys.stdout.write("\n")
    except ContourError as error:
        print(f"contour-config: ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
