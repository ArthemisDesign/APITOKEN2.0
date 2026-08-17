import type { LearnArticle } from "../learn";
import { quickSetupSteps } from "../learn-shared";

export const article: LearnArticle = {
  slug: "free-claude-api-key",
  cluster: "free",
  title: "Free Claude API Key to Get Started",
  h1: "Get a free Claude API key to start",
  description: "Create a Claude API key with Google or GitHub and get $5 of platform bonus credit — no card required, no Anthropic account, instant access.",
  keywords: ["free claude api key", "claude api free", "free claude api", "claude api free tier", "free anthropic api key", "claude api no card", "claude api no credit card", "claude api free credits", "try claude api free"],
  dek: "Create your account with Google or GitHub to receive $5 of platform bonus credit and make real calls before spending anything. Email and password accounts do not receive the bonus.",
  sections: [
    { h2: "What 'free' includes", blocks: [
      { type: "list", items: [
        "A working API key across all supported Claude models.",
        "A one-time $5 platform welcome bonus for new Google/GitHub accounts, no card required.",
        "Enough headroom to wire up your tools and run genuine requests.",
      ] },
      { type: "p", text: "When you are ready for more, top up any whole-dollar amount and your discount kicks in automatically." },
    ] },
    { h2: "How to claim it", blocks: [
      { type: "p", text: "Choose Google or GitHub when creating the account. Registering with email and password creates a usable account but does not grant the welcome bonus." },
      quickSetupSteps,
    ] },
    { h2: "Is the Claude API free forever?", blocks: [
      { type: "p", text: "The included $5 platform bonus is a free start, not an unlimited free tier. After it, you pay only for the tokens you use — there is no subscription and no monthly minimum, and your prepaid balance never expires." },
    ] },
  ],
  faq: [
    { q: "Is the free usage real API access?", a: "Yes. The $5 Google/GitHub platform bonus runs against the same supported models and endpoints as paid balance." },
    { q: "Do I need a card to start?", a: "No card is required. Create the account with Google or GitHub to receive the included $5 platform bonus." },
    { q: "Do I need a credit card for a free Claude API key?", a: "No. Create the account with Google or GitHub to receive the included $5 platform bonus without a card." },
  ],
  related: ["claude-api-free-trial", "how-to-buy-claude-api-key", "claude-code-without-subscription", "cheapest-claude-api"],
  updated: "2026-07-17",
};
