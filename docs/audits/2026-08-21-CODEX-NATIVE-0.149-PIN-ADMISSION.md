# Codex native 0.149 pin admission

- **Дата:** 2026-08-21
- **Target:** `CODEX_CLI_VERSION=0.149.0`
- **Official release:** `rust-v0.149.0`, commit `758ef40f50c1a458425c7cfbf1eb12cbc07af0b0`
- **Credential:** throwaway ChatGPT Pro, official `@openai/codex@0.149.0 login --device-auth`

## Admission evidence

The credential lived in an isolated mode-0700 `CODEX_HOME`, was never printed or committed, and was deleted
immediately after proof. Refresh-family rotation and WebSocket were not executed.

Exact private ChatGPT backend identity used:

- `originator: codex_cli_rs`;
- User-Agent `codex_cli_rs/0.149.0 …`;
- standalone `version: 0.149.0`;
- models query `client_version=0.149.0`;
- official session/thread/window/turn metadata and client metadata.

Results:

1. `/backend-api/codex/models` — HTTP 200, 9 entries; Luna supports current `priority` and legacy `fast`.
2. `/backend-api/wham/usage` — HTTP 200; Pro plan, request allowed, no reached quota wall.
3. One Luna Responses generation — HTTP 200, incremental SSE and `response.completed`.
4. Request controls accepted: `parallel_tool_calls:false`, reasoning `low/auto/all_turns`, `store:false`,
   `stream:true`, no tools.
5. Terminal usage: input 14, cached 0, output 6, reasoning 0, total 20.
6. Reviewed `x-codex-*` headers and `x-codex-turn-state` were present.
7. Requested priority reported completed tier `default`; this matches the documented ChatGPT-auth diagnostic
   behavior and does not prove a Fast downgrade.

## Safety and privacy

- No raw token, account id, user id, email, prompt output or metadata blob is persisted in this audit.
- The old probe exposed provider identity fields in local output; its usage projection is corrected to emit only
  plan/window/credit shape keys.
- The throwaway plaintext credential was deleted before the pin change.
- No refresh reuse test was run because it destructively rotates the family and is not required for wire identity.

## Verdict

Native ChatGPT backend identity 0.149 admission is **GREEN**. The compiled default can move from 0.146.0 to
0.149.0 together with tests, example config, provider contract and exact release evidence. WebSocket and remote
compaction remain separate optional capabilities and are not implied by this pin admission.
