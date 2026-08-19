import type { LearnArticle } from "../learn";
import { BASE, OPENAI_BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-key-security",
  cluster: "integrate",
  title: "Claude API Key Security: Storage and Rotation",
  h1: "Keep your Claude API key secure",
  description: "Claude API key security: storage rules, lifetime spending limits, the safe rotation order, and what to do in the first ten minutes after a leak.",
  keywords: ["claude api key security", "claude api key leaked", "rotate claude api key", "revoke claude api key", "anthropic api key best practices", "store api keys in environment variables", "api key rotation", "claude api key management", "secure anthropic api key", "api key secret manager"],
  updated: "2026-08-17",
  dek: "Claude API key security is mostly boring discipline: keep the key out of source control, cap what it can spend, and rehearse revocation before you need it. This guide walks through the exact controls apiToken.sale gives you — a lifetime spending limit, an expiration date, per-tool named keys — plus a storage setup and a leak-response playbook you can copy.",
  sections: [
    { h2: "What a stolen key can actually do", blocks: [
      { type: "p", text: `Your apiToken.sale key (it looks like ${KEY}) is a bearer credential: whoever presents it can run Claude, GPT, Gemini and Kimi requests against your prepaid balance. There is no second factor at request time — possession is permission. So the goal is not to make leaks impossible; it is to make them cheap, detectable and reversible.` },
      { type: "p", text: "Prepaid billing already bounds the worst case to your current balance, and the lifetime spending limit bounds it further. What remains is inconvenience and surprise: an attacker draining credit at 3 a.m., or a key sitting in a public repo for months because nobody noticed. Both are solved by the same three controls and a short rotation drill." },
    ] },
    { h2: "Three controls to set before the first request", blocks: [
      { type: "p", text: "Every key you create in the dashboard supports these settings. Configure them at creation time — retrofitting them after a leak is too late." },
      { type: "table", headers: ["Control", "What it does", "When to use it"], rows: [
        ["Lifetime spending limit", "Hard-stops the key once its total spend reaches a fixed amount, no matter who is using it", "Every key, always — set it to what the project should ever cost"],
        ["Expiration date", "Disables the key automatically on a date you choose", "Contractors, trials, demos, any temporary access"],
        ["Descriptive key name", "Tells you which tool and environment the key serves months later", "Every key — you will thank yourself during a 2 a.m. revocation"],
      ] },
      { type: "p", text: "Issue a separate key per tool and per environment instead of sharing one. Revoking a leaked Cursor key should never take down your production backend, and a dashboard full of keys named prod-backend, cursor-laptop and ci-staging makes the blast radius obvious at a glance." },
      { type: "link", text: "Not sure what limit to set? Price the workload first with the Claude API cost calculator.", href: "/tools/claude-api-cost-calculator" },
      cta(),
    ] },
    { h2: "Storage rules that survive contact with reality", blocks: [
      { type: "p", text: "One rule covers most of it: the key lives in an environment variable or a secret manager, never in source code. In practice that means a .env file that is git-ignored before the first commit, or a manager like 1Password CLI, Doppler or AWS Secrets Manager injecting the variable at run time." },
      { type: "code", code: `# .env — commit .env.example without values, never this file\nANTHROPIC_BASE_URL=${BASE}\nANTHROPIC_API_KEY=${KEY}\n\n# .gitignore — add this before the first commit, not after\n.env` },
      { type: "list", items: [
        "Git history — deleting the file in a follow-up commit does not remove the key; treat it as leaked and rotate.",
        "Client-side JavaScript — anything bundled into a browser app is public by definition; call the API from your backend.",
        "CI logs — echoing env vars in a pipeline step prints the key into build logs; mask secrets and never print them.",
        "Shell history — a raw curl -H \"x-api-key: sk-pool-…\" command saves the key to your history file; export it as a variable first.",
        "Chats and tickets — pasting a key into Slack, Telegram or an issue tracker leaves a permanent, searchable copy.",
      ] },
      { type: "note", text: "Screenshots and screen shares count too. If a key appeared on a recorded call or in a shared screenshot, rotate it — the ten minutes cost less than the alternative." },
    ] },
    { h2: "Wire it into your tools without hardcoding", blocks: [
      { type: "p", text: "Every major client reads credentials from the environment, so nothing needs to live in code:" },
      { type: "code", code: `# Anthropic SDK and Claude Code\nexport ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# OpenAI-compatible clients (GPT models, and Claude via the same lane)\nexport OPENAI_BASE_URL=${OPENAI_BASE}\nexport OPENAI_API_KEY=${KEY}` },
      { type: "p", text: "The Anthropic and OpenAI SDKs pick these variables up automatically — if you are passing a key as a string literal to the constructor, that is the smell to fix. On servers, inject the variable from your platform's secret store (hosting env settings, Docker secrets, a systemd EnvironmentFile) rather than baking it into an image or a config file." },
    ] },
    { h2: "Rotate in four moves", blocks: [
      { type: "p", text: "Rotation is cheap when you have practiced it once. This order keeps every client authenticated the whole way through:" },
      { type: "steps", items: [
        "Create the replacement key in the dashboard. Give it the same lifetime spending limit as the old one, an expiration date if the access is temporary, and a name that includes the date, like prod-backend-2026-08.",
        "Update the client: change the env var or secret value, then restart or redeploy so the process actually reloads it.",
        "Watch usage in the dashboard until requests are flowing on the new key and the old one has gone quiet.",
        "Revoke the old key. Only now — revoking first is how you manufacture an avoidable 401 outage.",
      ] },
      { type: "p", text: "How often should you rotate? On a fixed schedule only if your security policy demands one. Otherwise rotate when a tool is retired, a contractor rolls off, a laptop is lost, or you simply cannot account for every place a key was pasted. Uncertainty is itself a rotation trigger." },
    ] },
    { h2: "The first ten minutes after a leak", blocks: [
      { type: "steps", items: [
        "Revoke the exposed key in the dashboard immediately. This is the whole ballgame — new requests stop at once, and the lifetime spending limit caps whatever happened before you got there.",
        "Open the usage view and look for spend or models you do not recognize; that tells you when the leak started being used.",
        "Issue a replacement and update clients using the rotation order above.",
        "Fix the source: purge the key from git history, scrub CI logs, and rotate any other secret that lived in the same file or message.",
      ] },
      { type: "note", text: "If the key touched a public GitHub repo even briefly, assume it was harvested — automated scanners pick up new commits within minutes. \"I deleted it quickly\" is not a mitigation; revocation is." },
    ] },
  ],
  faq: [
    { q: "What happens if my Claude API key gets stolen?", a: "Whoever holds it can spend your prepaid balance on Claude, GPT, Gemini and Kimi models until you revoke it or it hits its lifetime spending limit. Revoke it in the dashboard first, investigate second." },
    { q: "Is the spending limit per day, per month, or lifetime?", a: "Lifetime: it caps the total a key can ever spend, and the balance itself is prepaid, so a leaked key can never run up open-ended charges. For temporary access, pair it with an expiration date." },
    { q: "Should I use one API key for all my tools?", a: "No. Issue a separate, clearly named key per tool and environment so revoking a leaked key never takes down unrelated clients, and dashboard usage shows exactly which tool spent what." },
    { q: "How do I rotate a Claude API key without breaking my app?", a: "Create the replacement key first, update the client and confirm traffic on the new key in the dashboard, then revoke the old one. Revoking before clients are updated just turns a rotation into an outage." },
    { q: "I committed my API key to GitHub. Is deleting the file enough?", a: "No — the key stays in git history, and public repos are scanned within minutes. Revoke and rotate the key, then scrub the history; deletion alone is no mitigation at all." },
  ],
  related: ["claude-api-best-practices", "claude-api-rate-limits", "claude-code-api-key", "how-billing-works"],
};
