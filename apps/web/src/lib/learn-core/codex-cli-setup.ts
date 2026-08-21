import type { LearnArticle } from "../learn";
import { cta, OPENAI_BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "codex-cli-setup",
  cluster: "integrate",
  title: "Codex CLI Setup: Custom GPT-5.6 Provider",
  h1: "Codex CLI setup: a custom provider profile for apiToken.sale",
  description: "Codex CLI setup without a ChatGPT login: one model_providers profile points Codex at apiToken.sale and runs GPT-5.6 on prepaid balance at 50% off.",
  keywords: ["codex cli setup", "codex custom model provider", "codex config.toml profile", "codex cli api key", "codex cli without chatgpt", "codex cli gpt-5.6", "codex responses api", "codex base_url", "openai codex cli config"],
  dek: "Codex CLI setup comes down to one TOML profile: declare a custom model provider, point base_url at apiToken.sale, and name the environment variable that holds your key. From there Codex runs GPT-5.6 models entirely on API-key authentication against your prepaid balance — no ChatGPT login, at a flat 50% below official OpenAI spend.",
  sections: [
    { h2: "Codex CLI does not need a ChatGPT account", blocks: [
      { type: "p", text: "Codex CLI authenticates however its active model provider tells it to. Define a custom provider in a model_providers table, export the API key that provider names, and Codex never looks at the ChatGPT login in auth.json — every request is signed with your key and billed by whoever owns the endpoint. Point that endpoint at apiToken.sale and each session draws on one prepaid balance, metered at official OpenAI token rates with a flat 50% B2C discount applied." },
      { type: "p", text: "The clean way to do this is a named profile rather than editing your main configuration. The profile lives in its own file, your default Codex setup and any existing ChatGPT login stay exactly as they were, and you opt in per run with a single flag. Delete the file and nothing about your environment remembers apiToken.sale existed." },
      cta(),
    ] },
    { h2: "Write the apitoken profile once", blocks: [
      { type: "p", text: "Save this as ~/.codex/apitoken.config.toml. It declares the provider, the endpoint, the wire protocol, and the environment variable Codex should read the secret from:" },
      { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "${OPENAI_BASE}"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
      { type: "p", text: "Two lines carry the security posture. env_key names a variable instead of storing the secret, so the key lives in your shell and never in a file you might commit. And base_url keeps its /v1 suffix — dropping it is the single most common cause of a broken first run, because every route Codex calls hangs off that prefix." },
      { type: "note", text: "Keep wire_api = \"responses\". Codex 0.149 accepts only the Responses wire. The gateway also serves Chat Completions for other clients, but that does not make wire_api = \"chat\" valid in Codex." },
    ] },
    { h2: "Export the key, check the catalog, run", blocks: [
      { type: "steps", items: [
        `Export the key in the shell that will launch Codex: export APITOKEN_API_KEY=${KEY} — put the same line in your shell profile if you want it permanent.`,
        `Confirm what is enabled before you guess model IDs: curl ${OPENAI_BASE}/models with the same Bearer key returns the live catalog.`,
        "Launch with the profile flag: codex --profile apitoken. Passing the flag explicitly removes any ambiguity about which provider — and which env var — is active for the session.",
        "Send one small prompt first. A clean answer proves the key, the base_url and the balance in a single round trip; a failure at this stage is cheap to diagnose.",
      ] },
      { type: "code", code: `export APITOKEN_API_KEY=${KEY}\n\ncurl ${OPENAI_BASE}/models \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY"\n\ncodex --profile apitoken` },
      { type: "note", text: "The catalog endpoint answers for the whole gateway, not just GPT: the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*). The same key and balance cover supported Claude, Gemini and Kimi models too — Codex will only ever call the provider its profile points at." },
    ] },
    { h2: "Pick the right GPT-5.6 tier for the session", blocks: [
      { type: "p", text: "The model line in the profile is your default, not a commitment — edit it per project. The three GPT-5.6 tiers exist because agentic coding burns tokens at very different rates depending on how hard the reasoning is:" },
      { type: "table", headers: ["Model ID", "Tier", "Official in / out ($ per 1M)", "Cached input"], rows: [
        ["gpt-5.6-sol", "Flagship", "$5 / $30", "$0.50"],
        ["gpt-5.6-terra", "Balanced", "$2 / $12", "$0.20"],
        ["gpt-5.6-luna", "Fast", "$0.20 / $1.20", "$0.02"],
      ] },
      { type: "list", items: [
        "gpt-5.6-sol for the hardest work: multi-file refactors, subtle debugging, anything where a wrong answer costs more than the tokens.",
        "gpt-5.6-terra as the daily driver — the tier most Codex sessions should default to.",
        "gpt-5.6-luna for fast cheap steps: boilerplate, renames, throwaway scripts, and high-volume loops where latency matters more than depth.",
        "Cached input is where agentic loops save real money — repeated context reads bill at the cached rate, then the 50% discount comes off on top.",
      ] },
      { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
    ] },
    { h2: "The four errors Codex will actually show you", blocks: [
      { type: "list", items: [
        "Missing APITOKEN_API_KEY — the variable named by env_key is not exported in the shell that runs codex. Export it in that same shell, or in your shell profile, and retry.",
        "stream error: unexpected status 401 — the key is wrong, revoked, or the base_url lost its /v1 suffix. Reproduce the call with curl outside Codex to isolate which half is broken.",
        "stream error: unexpected status 404 — the model ID is not enabled. Check GET " + OPENAI_BASE + "/models instead of assuming the ID you typed exists.",
        "402 — the shared prepaid balance needs a top-up. Backoff will not fix it; add balance and the next request succeeds.",
      ] },
      { type: "p", text: "All four are configuration or balance problems, not model problems — none of them is solved by retrying the same command. The 401 in particular almost always reduces to the /v1 suffix or an extra character pasted into the key." },
      { type: "link", text: "The full Codex error playbook — config.toml, auth.json, stream errors", href: "/errors/codex" },
    ] },
    { h2: "What a Codex session costs on prepaid balance", blocks: [
      { type: "p", text: "Billing is per token at official OpenAI rates, with your flat 50% B2C discount subtracted before the charge touches the prepaid balance — the same rule that applies to Claude usage on the platform. There is no subscription and no seat fee: an idle week costs nothing, and a heavy session costs exactly the tokens it consumed at half the official spend." },
      { type: "p", text: "Because the balance is shared across supported Claude, GPT, Gemini and Kimi models, Codex sessions draw from the same pool as everything else you run. Watch usage in the dashboard, and treat a 402 as the signal it is — the meter ran out, nothing else broke." },
    ] },
  ],
  faq: [
    { q: "Do I need a ChatGPT account or subscription for Codex CLI?", a: "No. With a custom model_providers profile and the provider's API key in the environment, Codex runs entirely on API-key authentication — the ChatGPT login in auth.json is irrelevant." },
    { q: "Does this profile change my default Codex setup?", a: "No. The profile lives in its own file and activates only when you pass --profile apitoken. Your default configuration and any ChatGPT login stay untouched." },
    { q: "Is the GPT-5.6 discount the same as the Claude one?", a: "Yes. GPT-5.6 usage is metered at official OpenAI token rates and your flat 50% B2C discount applies to the same prepaid balance." },
    { q: "Which wire_api value does Codex 0.149 use?", a: "Use wire_api = \"responses\". Codex 0.149 accepts only the Responses wire. Chat Completions is available to other clients through the gateway, but wire_api = \"chat\" is not a valid Codex setting." },
    { q: "Can I switch GPT-5.6 models without editing the profile?", a: "The model line in the profile sets the default; editing it per project is the supported way to move between gpt-5.6-sol, gpt-5.6-terra and gpt-5.6-luna." },
    { q: "What does a 402 error mean mid-session?", a: "The shared prepaid balance is empty and needs a top-up. Retrying with backoff will not help — add balance and the next request goes through." },
  ],
  related: ["openai-api-quickstart", "gpt-5-6-sol-vs-terra-vs-luna", "gpt-api-pricing", "how-billing-works"],
  published: "2026-07-29",
  updated: "2026-08-17",
};
