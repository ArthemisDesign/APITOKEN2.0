import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-free-trial",
  cluster: "free",
  title: "Claude API Free Trial — Start in Minutes",
  h1: "Try the Claude API free",
  description: "Start coding with Claude in minutes. New accounts created with Google or GitHub get $5 of platform bonus credit, with no card required.",
  keywords: ["claude api free trial", "try claude api", "claude api test", "claude api sandbox", "claude api demo", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "There is no separate trial to apply for — create the account with Google or GitHub to get $5 of platform bonus credit and run real calls against every supported model.",
  sections: [
    { h2: "Prove it before you pay", blocks: [
      { type: "p", text: "The included usage is designed to check the gateway end to end: create a key, connect your editor, and confirm streaming, tool use and your favorite model all behave as expected." },
      cta(),
    ] },
    { h2: "Then scale on your terms", blocks: [
      { type: "p", text: "When the trial usage runs low, top up any amount. There is no subscription and balance never expires, so you only ever pay for what you actually call." },
    ] },
  ],
  faq: [
    { q: "How do I start the trial?", a: "Create a new account with Google or GitHub. The $5 platform bonus is added automatically; email and password accounts are not eligible." },
    { q: "What happens when the free usage runs out?", a: "Top up any whole-dollar amount to keep going; your flat discount applies immediately." },
  ],
  related: ["free-claude-api-key", "claude-api-without-waitlist", "claude-api-quick-setup", "claude-haiku-api"],
};
