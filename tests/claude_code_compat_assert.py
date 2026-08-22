#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--evidence-file", required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--require-discovery", action="store_true")
args = parser.parse_args()
entries = [json.loads(line) for line in Path(args.evidence_file).read_text().splitlines() if line]

messages = [entry for entry in entries if entry["method"] == "POST"
            and entry["path"].startswith("/v1/messages")
            and "count_tokens" not in entry["path"]]
if not messages:
    raise SystemExit("no Claude Code Messages request captured")

current_control_requests = []
for entry in messages:
    headers = entry["headers"]
    body = entry["body"]
    if entry["path"] != "/v1/messages?beta=true":
        raise SystemExit(f"unexpected Messages path: {entry['path']}")
    if headers.get("user-agent") != [f"claude-cli/{args.version} (external, sdk-cli)"]:
        raise SystemExit(f"wrong User-Agent: {headers.get('user-agent')}")
    if headers.get("anthropic-version") != ["2023-06-01"]:
        raise SystemExit("anthropic-version drift")
    betas = set((headers.get("anthropic-beta") or [""])[0].split(","))
    required = {"claude-code-20250219", "effort-2025-11-24"}
    if not required.issubset(betas):
        raise SystemExit(f"missing baseline beta controls: {sorted(required - betas)}")
    if not isinstance(body, dict):
        raise SystemExit("Messages body is not an object")
    baseline_keys = {"model", "messages", "system", "stream", "tools", "thinking",
                     "output_config", "metadata", "max_tokens"}
    if not baseline_keys.issubset(body):
        raise SystemExit(f"missing baseline body fields: {sorted(baseline_keys - set(body))}")
    system = body.get("system") or []
    billing = system[0].get("text", "") if system and isinstance(system[0], dict) else ""
    if f"cc_version={args.version}." not in billing:
        raise SystemExit(f"billing attribution version drift: {billing[:120]}")
    if "context_management" in body:
        current_control_requests.append((entry, betas))

if not current_control_requests:
    raise SystemExit("no main Claude Code request carried current context controls")
for entry, betas in current_control_requests:
    body = entry["body"]
    if "context-management-2025-06-27" not in betas:
        raise SystemExit("context-management beta missing")
    if body.get("thinking", {}).get("type") != "adaptive":
        raise SystemExit("adaptive thinking missing")
    if body.get("output_config", {}).get("effort") != "low":
        raise SystemExit("low effort control missing")
    edits = body.get("context_management", {}).get("edits")
    if edits != [{"type": "clear_thinking_20251015", "keep": "all"}]:
        raise SystemExit(f"context management drift: {edits}")

if args.require_discovery:
    discovery = [entry for entry in entries
                 if entry["method"] == "GET" and entry["path"] == "/v1/models?limit=1000"]
    if len(discovery) != 1:
        raise SystemExit(f"expected one discovery request, got {len(discovery)}")
    if discovery[0]["headers"].get("user-agent") != [f"claude-code/{args.version}"]:
        raise SystemExit("discovery User-Agent drift")

print(f"Claude Code {args.version} exact wire contract passed")
