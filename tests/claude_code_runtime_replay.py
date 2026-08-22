#!/usr/bin/env python3
"""Replay one exact captured Claude Code request through project engine/router runtime."""

import argparse
import http.client
import json
from pathlib import Path
from urllib.parse import urlparse

parser = argparse.ArgumentParser()
parser.add_argument("--evidence-file", required=True)
parser.add_argument("--base-url", required=True)
parser.add_argument("--api-key", required=True)
args = parser.parse_args()

entries = [json.loads(line) for line in Path(args.evidence_file).read_text().splitlines() if line]
requests = [entry for entry in entries if entry["method"] == "POST"
            and entry["path"] == "/v1/messages?beta=true"
            and isinstance(entry["body"], dict)
            and "context_management" in entry["body"]]
if not requests:
    raise SystemExit("no exact main Claude Code request available for runtime replay")
entry = requests[-1]
origin = urlparse(args.base_url)
if origin.scheme != "http" or origin.hostname not in {"127.0.0.1", "localhost"}:
    raise SystemExit("runtime replay accepts only an explicit loopback HTTP origin")
body = json.dumps(entry["body"], separators=(",", ":")).encode()
headers = {
    "content-type": "application/json",
    "x-api-key": args.api_key,
    "anthropic-version": (entry["headers"].get("anthropic-version") or ["2023-06-01"])[0],
    "anthropic-beta": (entry["headers"].get("anthropic-beta") or [""])[0],
}
conn = http.client.HTTPConnection(origin.hostname, origin.port, timeout=60)
conn.request("POST", entry["path"], body=body, headers=headers)
response = conn.getresponse()
payload = response.read()
conn.close()
if response.status != 200:
    raise SystemExit(f"runtime replay returned HTTP {response.status}: {payload[:300]!r}")
text = payload.decode("utf-8", "replace")
for marker in ["event: message_start", "event: message_stop"]:
    if marker not in text:
        raise SystemExit(f"runtime replay lacks {marker}")
print(f"runtime replay passed through {args.base_url}")
