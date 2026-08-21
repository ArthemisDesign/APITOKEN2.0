#!/usr/bin/env python3
"""Live wire probe for the native Codex (ChatGPT) transport.

Run on a throwaway ChatGPT account (Plus/Pro) BEFORE enabling the native provider in
production. The probe completes the official device flow, then verifies the exact wire facts
the Rust transport depends on, and prints them REDACTED (no tokens, no account ids):

  1. `GET /backend-api/codex/models`: model availability plus current/legacy Fast metadata;
  2. `POST /backend-api/codex/responses` HTTP+SSE liveness: requested tier, backend-reported
     response tier, SSE event types, and rate-limit response headers;
  3. `GET /backend-api/wham/usage`: JSON shape (plan_type, rate_limit primary/secondary,
     resets spelling);
  4. refresh-token rotation: two consecutive refreshes must succeed, and the second one must
     reject the FIRST refresh token (strict family reuse detection);
  5. optional WebSocket reachability check for `wss://chatgpt.com/backend-api/codex/responses`.

Usage (safe default: no generation, no refresh rotation):
    python3 tools/codex-native/probe-live.py --service-tier priority --no-ws

One paid micro-turn on a throwaway profile only:
    python3 tools/codex-native/probe-live.py --service-tier priority --no-ws \
      --execute-paid-turn --max-nanousd 100000 --confirm-paid-budget 100000

Findings belong in research/CODEX_NATIVE_WIRE.md. Never commit the captured tokens or ids.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"
TOKEN_URL = "https://auth.openai.com/oauth/token"
USERCODE_URL = "https://auth.openai.com/api/accounts/deviceauth/usercode"
DEVICETOKEN_URL = "https://auth.openai.com/api/accounts/deviceauth/token"
VERIFICATION_URL = "https://auth.openai.com/codex/device"
BASE_URL = "https://chatgpt.com/backend-api/codex"
USAGE_URL = "https://chatgpt.com/backend-api/wham/usage"
CLI_VERSION = "0.149.0"
DEFAULT_MAX_NANOUSD = 100_000
ORIGINATOR = "codex_cli_rs"

TIMEOUT = 30


def make_opener(proxy):
    if proxy:
        return urllib.request.build_opener(
            urllib.request.ProxyHandler({"http": proxy, "https": proxy})
        )
    return urllib.request.build_opener()


def post_json(opener, url, body, headers=None):
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", **(headers or {})},
        method="POST",
    )
    return opener.open(request, timeout=TIMEOUT)


def device_login(opener):
    response = post_json(opener, USERCODE_URL, {"client_id": CLIENT_ID})
    payload = json.loads(response.read())
    device_auth_id = payload["device_auth_id"]
    user_code = payload["user_code"]
    interval = max(int(payload.get("interval", 5)), 1)
    print(f"== device flow: open {VERIFICATION_URL} and enter the one-time code")
    print(f"   code: {user_code} (valid ~15 minutes)")
    deadline = time.time() + 15 * 60
    while time.time() < deadline:
        time.sleep(interval)
        try:
            response = post_json(
                opener,
                DEVICETOKEN_URL,
                {"device_auth_id": device_auth_id, "user_code": user_code},
            )
            token = json.loads(response.read())
            print("== device flow approved")
            return token
        except urllib.error.HTTPError as error:
            body = error.read().decode(errors="replace")
            if "authorization_pending" in body:
                continue
            if "slow_down" in body:
                interval += 5
                continue
            raise SystemExit(f"device flow failed: HTTP {error.code}: {body[:200]}")
    raise SystemExit("device flow expired")


def exchange_code(opener, token):
    # Device flow returns authorization_code + code_verifier; exchange for the token set.
    body = {
        "grant_type": "authorization_code",
        "client_id": CLIENT_ID,
        "code": token["authorization_code"],
        "code_verifier": token["code_verifier"],
        "redirect_uri": "https://auth.openai.com/codex/device",
    }
    request = urllib.request.Request(
        TOKEN_URL,
        data=urllib.parse.urlencode(body).encode(),
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        method="POST",
    )
    return json.loads(opener.open(request, timeout=TIMEOUT).read())


def account_id_of(tokens):
    claims = jwt_claims(tokens.get("id_token", ""))
    auth = claims.get("https://api.openai.com/auth", {})
    return auth.get("chatgpt_account_id", "")


def jwt_claims(token):
    import base64

    try:
        payload = token.split(".")[1]
        payload += "=" * (-len(payload) % 4)
        return json.loads(base64.urlsafe_b64decode(payload))
    except Exception:
        return {}


def request_identity():
    thread = str(uuid.uuid4())
    return {
        "installation": str(uuid.uuid4()),
        # Root Codex sessions use their thread UUID as the session UUID.
        "session": thread,
        "thread": thread,
        "turn": str(uuid.uuid4()),
        "window": str(uuid.uuid4()),
    }


def auth_headers(tokens, client_version):
    return {
        "Authorization": f"Bearer {tokens['access_token']}",
        "ChatGPT-Account-ID": account_id_of(tokens),
        "originator": ORIGINATOR,
        "User-Agent": f"{ORIGINATOR}/{client_version} (Linux; x86_64) {ORIGINATOR}",
        "version": client_version,
    }


def turn_metadata(identity):
    return json.dumps({
        "installation_id": identity["installation"],
        "session_id": identity["session"],
        "thread_id": identity["thread"],
        "turn_id": identity["turn"],
        "window_id": identity["window"],
        "request_kind": "turn",
    }, separators=(",", ":"))


def turn_headers(tokens, client_version, identity):
    metadata = turn_metadata(identity)
    return {
        **auth_headers(tokens, client_version),
        "session-id": identity["session"],
        "thread-id": identity["thread"],
        "x-client-request-id": identity["thread"],
        "x-codex-window-id": identity["window"],
        "x-codex-turn-metadata": metadata,
    }


def probe_models(opener, tokens, client_version, model):
    query = urllib.parse.urlencode({"client_version": client_version})
    request = urllib.request.Request(
        f"{BASE_URL}/models?{query}",
        headers=auth_headers(tokens, client_version),
    )
    try:
        response = opener.open(request, timeout=TIMEOUT)
    except urllib.error.HTTPError as error:
        print(f"== models: HTTP {error.code}: {error.read(200).decode(errors='replace')}")
        return None
    payload = json.loads(response.read())
    entries = payload.get("models", payload.get("data", []))
    rows = []
    for entry in entries:
        if isinstance(entry, str):
            model_id, current, legacy = entry, [], []
        else:
            model_id = entry.get("id") or entry.get("model") or entry.get("slug")
            current = [
                tier.get("id") if isinstance(tier, dict) else tier
                for tier in entry.get("service_tiers", [])
            ]
            legacy = entry.get("additional_speed_tiers", [])
        if model_id:
            rows.append({"model": model_id, "service_tiers": current, "legacy_speed_tiers": legacy})
    selected = next((row for row in rows if row["model"] == model), None)
    print(f"== models: HTTP {response.status}; client_version={client_version}; entries={len(rows)}")
    print(f"   selected model tiers: {json.dumps(selected, sort_keys=True)}")
    return selected


def probe_responses(opener, tokens, client_version, model, service_tier, identity):
    metadata = turn_metadata(identity)
    body = {
        "model": model,
        "instructions": "",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "Reply with the single word PONG."}],
            }
        ],
        "tools": [],
        "tool_choice": "auto",
        "parallel_tool_calls": True,
        "reasoning": {"effort": "low", "summary": "auto"},
        "store": False,
        "stream": True,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": identity["session"],
        "client_metadata": {
            "x-codex-installation-id": identity["installation"],
            "session_id": identity["session"],
            "thread_id": identity["thread"],
            "turn_id": identity["turn"],
            "x-codex-window-id": identity["window"],
            "x-codex-turn-metadata": metadata,
        },
    }
    if service_tier != "none":
        body["service_tier"] = service_tier
    request = urllib.request.Request(
        f"{BASE_URL}/responses",
        data=json.dumps(body).encode(),
        headers={
            **turn_headers(tokens, client_version, identity),
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        },
        method="POST",
    )
    try:
        response = opener.open(request, timeout=120)
    except urllib.error.HTTPError as error:
        print(f"== responses: HTTP {error.code}")
        print(f"   error headers: {dict(error.headers)}")
        print(f"   error body (truncated): {error.read(300).decode(errors='replace')}")
        return False, None
    print(f"== responses: HTTP {response.status}")
    interesting = {
        name: value
        for name, value in response.headers.items()
        if name.lower().startswith(("x-codex", "x-ratelimit", "openai", "retry-after"))
    }
    print(f"   rate-limit headers seen: {json.dumps(interesting, indent=2)}")
    events, event_name, data_lines = [], "", []
    served_tier = None

    def dispatch():
        nonlocal served_tier
        if not data_lines:
            return
        try:
            payload = json.loads("\n".join(data_lines))
        except json.JSONDecodeError:
            return
        kind = payload.get("type") or event_name
        if kind:
            events.append(kind)
        if kind == "response.completed":
            served_tier = payload.get("response", {}).get("service_tier")

    for raw in response:
        line = raw.decode(errors="replace").rstrip("\r\n")
        if not line:
            dispatch()
            event_name, data_lines = "", []
        elif line.startswith("event:"):
            event_name = line.split(":", 1)[1].strip()
        elif line.startswith("data:"):
            data_lines.append(line.split(":", 1)[1].lstrip())
    dispatch()
    print(f"   SSE event order: {events[:40]}")
    print(f"   requested service_tier: {service_tier}; served service_tier: {served_tier!r}")
    return True, served_tier


def probe_usage(opener, tokens, client_version):
    request = urllib.request.Request(USAGE_URL, headers=auth_headers(tokens, client_version))
    try:
        response = opener.open(request, timeout=TIMEOUT)
    except urllib.error.HTTPError as error:
        print(f"== wham/usage: HTTP {error.code}: {error.read(200).decode(errors='replace')}")
        return
    payload = json.loads(response.read())
    redacted = {
        key: ("<redacted>" if "email" in key.lower() else value)
        for key, value in payload.items()
    }
    print(f"== wham/usage: HTTP {response.status}")
    print(json.dumps(redacted, indent=2)[:2000])


def probe_refresh_rotation(opener, tokens):
    def refresh(refresh_token):
        body = {
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }
        request = urllib.request.Request(
            TOKEN_URL,
            data=urllib.parse.urlencode(body).encode(),
            headers={"Content-Type": "application/x-www-form-urlencoded"},
            method="POST",
        )
        return opener.open(request, timeout=TIMEOUT)

    first = json.loads(refresh(tokens["refresh_token"]).read())
    rotated = first.get("refresh_token") != tokens["refresh_token"]
    print(f"== refresh #1 ok; refresh_token rotated: {rotated}")
    second = json.loads(refresh(first["refresh_token"]).read())
    print("== refresh #2 ok (rotated token accepted)")
    try:
        refresh(tokens["refresh_token"])
        print("!! WARNING: the ORIGINAL refresh token was accepted after rotation — "
              "no strict family reuse detection observed")
    except urllib.error.HTTPError as error:
        print(f"== original refresh token rejected after rotation (HTTP {error.code}) — expected")
    return second


def probe_ws(tokens):
    import socket
    import ssl

    print("== ws reachability: opening TLS to chatgpt.com:443 (full WS handshake needs a client)")
    try:
        with socket.create_connection(("chatgpt.com", 443), timeout=10) as sock:
            with ssl.create_default_context().wrap_socket(
                sock, server_hostname="chatgpt.com"
            ) as tls:
                print(f"   TLS ok: {tls.version()}, ALPN={tls.selected_alpn_protocol()}")
    except OSError as error:
        print(f"   TLS failed: {error}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proxy", default=None)
    parser.add_argument("--client-version", default=CLI_VERSION)
    parser.add_argument("--model", default="gpt-5.6-sol")
    parser.add_argument(
        "--service-tier", choices=["none", "priority", "fast"], default="priority"
    )
    parser.add_argument(
        "--expect-served-tier",
        choices=["default", "priority", "flex"],
        default=None,
        help="exit non-zero unless response.completed reports this exact tier (diagnostic only)",
    )
    parser.add_argument(
        "--execute-paid-turn",
        action="store_true",
        help="allow exactly one paid Responses generation after the free checks",
    )
    parser.add_argument(
        "--confirm-paid-budget",
        type=int,
        default=0,
        help="required with --execute-paid-turn; exact aggregate nanoUSD authorization",
    )
    parser.add_argument(
        "--max-nanousd",
        type=int,
        default=DEFAULT_MAX_NANOUSD,
        help="hard aggregate paid-turn ceiling (default: 100000 nanoUSD)",
    )
    parser.add_argument(
        "--rotate-refresh-family",
        action="store_true",
        help="destructive throwaway-profile-only refresh rotation proof",
    )
    parser.add_argument("--no-ws", action="store_true")
    args = parser.parse_args()
    if args.max_nanousd <= 0 or args.max_nanousd > DEFAULT_MAX_NANOUSD:
        parser.error(f"--max-nanousd must be 1-{DEFAULT_MAX_NANOUSD}")
    if args.execute_paid_turn and args.confirm_paid_budget != args.max_nanousd:
        parser.error("paid turn requires --confirm-paid-budget equal to --max-nanousd")
    if not args.execute_paid_turn and args.confirm_paid_budget:
        parser.error("--confirm-paid-budget requires --execute-paid-turn")
    opener = make_opener(args.proxy)
    device = device_login(opener)
    tokens = exchange_code(opener, device)
    plan = jwt_claims(tokens.get("id_token", "")).get("https://api.openai.com/auth", {})
    print(f"== plan claim: {plan.get('chatgpt_plan_type')!r} (account id redacted)")
    identity = request_identity()
    probe_models(opener, tokens, args.client_version, args.model)
    ok, served_tier = True, None
    if args.execute_paid_turn:
        print(f"== paid turn authorized: cap={args.max_nanousd} nanoUSD; exactly one dispatch")
        ok, served_tier = probe_responses(
            opener,
            tokens,
            args.client_version,
            args.model,
            args.service_tier,
            identity,
        )
    else:
        print("== paid turn skipped (add --execute-paid-turn and exact --confirm-paid-budget)")
    probe_usage(opener, tokens, args.client_version)
    if args.rotate_refresh_family:
        tokens = probe_refresh_rotation(opener, tokens)
    else:
        print("== refresh-family rotation skipped")
    if not args.no_ws:
        probe_ws(tokens)
    if args.expect_served_tier and not args.execute_paid_turn:
        print("!! --expect-served-tier requires --execute-paid-turn")
        return 2
    if args.expect_served_tier and served_tier != args.expect_served_tier:
        print(
            f"!! served-tier mismatch: expected {args.expect_served_tier!r}, "
            f"got {served_tier!r}"
        )
        return 1
    if args.service_tier == "priority" and served_tier != "priority":
        print("!! backend reported a non-priority response tier; for ChatGPT-auth Codex this is "
              "not proof of a Fast downgrade (openai/codex#14204, #30413, #32191)")
    print("== probe complete; record findings in research/CODEX_NATIVE_WIRE.md")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
