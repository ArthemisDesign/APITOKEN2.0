import type { LearnArticle } from "../learn";
import { quickSetupSteps, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-without-waitlist",
  cluster: "buy",
  title: "Claude API With No Waitlist or Approval",
  h1: "Claude API access with no waitlist",
  description: "Skip the Anthropic waitlist and approval. Create an account on apiToken.sale, generate a Claude API key, and make your first call in minutes.",
  keywords: ["claude api no waitlist", "claude api instant access", "claude api without approval", "get claude api key fast", "claude api no anthropic account", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "Waiting for approval kills momentum. apiToken.sale gives you instant, self-serve access to every supported Claude model — no queue, no sales call, no company verification.",
  sections: [
    { h2: "Instant, self-serve access", blocks: [ quickSetupSteps, cta() ] },
    { h2: "What 'instant' actually means", blocks: [
      { type: "p", text: "The moment you generate a key it is live. There is no manual review step between signing up and your first successful request, so you can wire up a tool and ship in the same sitting." },
    ] },
    { h2: "From zero to first call", blocks: [
      { type: "list", items: [
        "Sign up and open the dashboard — no approval step.",
        "Generate a key and point your tool at router.apitoken.sale.",
        "Send a request and see it metered in your usage.",
      ] },
      { type: "p", text: "New accounts created with Google or GitHub also start with $5 of platform bonus credit, so you can validate the whole flow before topping up." },
    ] },
  ],
  faq: [
    { q: "Is there really no waitlist?", a: "Correct. Access is self-serve and instant — you generate a key and it works on the next request." },
    { q: "Do I need to talk to sales?", a: "No. B2C access is fully self-serve. Only negotiated B2B volume pricing involves a conversation." },
  ],
  related: ["how-to-buy-claude-api-key", "claude-api-quick-setup", "claude-api-activation-time", "free-claude-api-key"],
};
