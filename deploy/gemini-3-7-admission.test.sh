#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

python3 -I -P -B -S - "$ROOT" <<'PY'
import hashlib
import re
import shlex
import sys
from pathlib import Path

root = Path(sys.argv[1])
infrastructure = root / "deploy/watchdog-infrastructure.sh"
pins = {
    "DIRECT_ADMISSION_GATE_SHA256": root / "deploy/gemini-3-7-admission-gate.sh",
    "DIRECT_ADMISSION_TRANSPORT_SHA256": root / "deploy/gemini-3-7-admission-transport.py",
    "DIRECT_ADMISSION_PACKAGE_SHA256": root / "tools/gemini_calibration/__init__.py",
    "DIRECT_ADMISSION_STATE_SHA256": root / "tools/gemini_calibration/admission.py",
    "DIRECT_ADMISSION_RUN_LIVE_SHA256": root / "tools/gemini_calibration/run_live.py",
    "DIRECT_ADMISSION_UNIT_SHA256": root / "systemd/claude-api-gemini-3-7-admission.service",
}
assignments: dict[str, list[str]] = {name: [] for name in pins}
for line in infrastructure.read_text(encoding="utf-8").splitlines():
    match = re.fullmatch(r"([A-Z0-9_]+)=([0-9a-f]{64})", line)
    if match and match.group(1) in assignments:
        assignments[match.group(1)].append(match.group(2))
for name, path in pins.items():
    values = assignments[name]
    if len(values) != 1:
        raise SystemExit(f"{name} must have exactly one canonical outer digest pin")
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if values[0] != actual:
        raise SystemExit(f"{name} does not authenticate {path.relative_to(root)}")

unit = pins["DIRECT_ADMISSION_UNIT_SHA256"]
gate = pins["DIRECT_ADMISSION_GATE_SHA256"]
gate_text = gate.read_text(encoding="utf-8")
unit_stop_values = re.findall(
    r"^TimeoutStopSec=([0-9]+)$", unit.read_text(encoding="utf-8"), re.MULTILINE
)
gate_stop_values = re.findall(
    r"^CANARY_STOP_TIMEOUT_SEC=([0-9]+)$",
    gate_text,
    re.MULTILINE,
)
if len(unit_stop_values) != 1 or len(gate_stop_values) != 1:
    raise SystemExit("Gemini admission shutdown bounds must be singular integer seconds")
if int(gate_stop_values[0]) < int(unit_stop_values[0]) + 30:
    raise SystemExit("Gemini admission controller can truncate the systemd shutdown ladder")
cleanup_stop_calls = re.findall(
    r'^\s*if ! \$TIMEOUT "\$CANARY_STOP_TIMEOUT_SEC" '
    r'\$SYSTEMCTL stop "\$UNIT" >/dev/null 2>&1; then$',
    gate_text,
    re.MULTILINE,
)
if len(cleanup_stop_calls) != 1:
    raise SystemExit("Gemini admission cleanup does not use its authenticated shutdown bound")

exec_lines = [
    line.removeprefix("ExecStart=")
    for line in unit.read_text(encoding="utf-8").splitlines()
    if line.startswith("ExecStart=")
]
if len(exec_lines) != 1:
    raise SystemExit("Gemini admission unit must have exactly one ExecStart")
tokens = shlex.split(exec_lines[0])
override_tokens = [token for token in tokens if token.startswith("CLAUDE_API_TARIFF_OVERRIDES=")]
binary = (
    "/usr/local/lib/apitoken-watchdog/producers/"
    "264363f7838ddd2d156b14668a320047ad33b6ee/claude-api"
)
if (
    not tokens
    or tokens[0] != "/usr/bin/env"
    or override_tokens != ["CLAUDE_API_TARIFF_OVERRIDES=0"]
    or binary not in tokens
    or tokens.index("CLAUDE_API_TARIFF_OVERRIDES=0") > tokens.index(binary)
):
    raise SystemExit("Gemini admission unit does not pin compiled-only tariff authority")
PY

PYTHONPATH="$ROOT" python3 -P -B -S -m unittest \
  tools.gemini_calibration.test_run_live \
  tools.gemini_calibration.test_admission
python3 -I -P -B -S "$ROOT/deploy/gemini-3-7-admission-gate.test.py"
python3 -I -P -B -S "$ROOT/deploy/gemini-3-7-admission-transport.test.py"
