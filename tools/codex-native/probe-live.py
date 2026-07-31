#!/usr/bin/env python3
"""Live wire probe for the native Codex (ChatGPT) transport.

Run on a throwaway ChatGPT account (Plus/Pro) BEFORE enabling the native provider in
production. The probe completes the official device flow, then verifies the exact wire facts
the Rust transport depends on, and prints them REDACTED (no tokens, no account ids):

  1. `POST /backend-api/codex/responses` HTTP+SSE liveness: status, SSE event types in order,
     rate-limit response headers (exact `x-codex-*` names observed);
  2. `GET /backend-api/wham/usage`: JSON shape (plan_type, rate_limit primary/secondary,
     resets spelling);
  3. refresh-token rotation: two consecutive refreshes must succeed, and the second one must
     reject the FIRST refresh token (strict family reuse detection);
  4. optional WebSocket reachability check for `wss://chatgpt.com/backend-api/codex/responses`.

Usage:
    python3 tools/codex-native/probe-live.py [--proxy http://user:pass@host:port] [--no-ws]

Findings belong in research/CODEX_NATIVE_WIRE.md. Never commit the captured tokens or ids.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request

CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"
TOKEN_URL = "https://auth.openai.com/oauth/token"
USERCODE_URL = "https://auth.openai.com/api/accounts/deviceauth/usercode"
DEVICETOKEN_URL = "https://auth.openai.com/api/accounts/deviceauth/token"
VERIFICATION_URL = "https://auth.openai.com/codex/device"
BASE_URL = "https://chatgpt.com/backend-api/codex"
USAGE_URL = "https://chatgpt.com/backend-api/wham/usage"
CLI_VERSION = "0.145.0"
ORIGINATOR = "codex_cli_rs"
USER_AGENT = f"{ORIGINATOR}/{CLI_VERSION} (Linux; x86_64) {ORIGINATOR}"

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


def auth_headers(tokens):
    return {
        "Authorization": f"Bearer {tokens['access_token']}",
        "ChatGPT-Account-ID": account_id_of(tokens),
        "originator": ORIGINATOR,
        "User-Agent": USER_AGENT,
        "session_id": "00000000-0000-4000-8000-0000000000ab",
        "OpenAI-Beta": "responses=experimental",
    }


def probe_responses(opener, tokens):
    body = {
        "model": "gpt-5.5",
        "instructions": "",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "Reply with the single word PONG."}],
            }
        ],
        "store": False,
        "stream": True,
    }
    request = urllib.request.Request(
        f"{BASE_URL}/responses",
        data=json.dumps(body).encode(),
        headers={
            **auth_headers(tokens),
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
        return False
    print(f"== responses: HTTP {response.status}")
    interesting = {
        name: value
        for name, value in response.headers.items()
        if name.lower().startswith(("x-codex", "x-ratelimit", "openai", "retry-after"))
    }
    print(f"   rate-limit headers seen: {json.dumps(interesting, indent=2)}")
    events = []
    for raw in response:
        line = raw.decode(errors="replace").strip()
        if line.startswith("event:"):
            events.append(line.split(":", 1)[1].strip())
        if len(events) >= 40:
            break
    print(f"   SSE event order: {events}")
    return True


def probe_usage(opener, tokens):
    request = urllib.request.Request(USAGE_URL, headers=auth_headers(tokens))
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
    parser.add_argument("--no-ws", action="store_true")
    args = parser.parse_args()
    opener = make_opener(args.proxy)
    device = device_login(opener)
    tokens = exchange_code(opener, device)
    plan = jwt_claims(tokens.get("id_token", "")).get("https://api.openai.com/auth", {})
    print(f"== plan claim: {plan.get('chatgpt_plan_type')!r} (account id redacted)")
    probe_responses(opener, tokens)
    probe_usage(opener, tokens)
    tokens = probe_refresh_rotation(opener, tokens)
    if not args.no_ws:
        probe_ws(tokens)
    print("== probe complete; record findings in research/CODEX_NATIVE_WIRE.md")


if __name__ == "__main__":
    sys.exit(main())
