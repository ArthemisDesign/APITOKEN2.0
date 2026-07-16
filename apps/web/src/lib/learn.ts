// Learn cluster — programmatic SEO landing pages under /docs/learn.
// Each article targets one commercial search intent for apiToken.sale and is
// rendered server-side (fully crawlable) with Article + FAQPage + Breadcrumb
// structured data. Copy is grounded in real product facts only.

export type LearnCluster = "buy" | "free" | "integrate" | "compare" | "explain";

export type LearnBlock =
  | { type: "p"; text: string }
  | { type: "list"; items: string[] }
  | { type: "steps"; items: string[] }
  | { type: "code"; code: string }
  | { type: "note"; text: string };

export type LearnSection = { h2: string; blocks: LearnBlock[] };

export type LearnFaq = { q: string; a: string };

export type LearnArticle = {
  slug: string;
  cluster: LearnCluster;
  /** <title> without the brand suffix (appended by metadata template). */
  title: string;
  h1: string;
  description: string;
  keywords: string[];
  dek: string;
  sections: LearnSection[];
  faq: LearnFaq[];
  related: string[];
};

export const clusterLabels: Record<LearnCluster, { label: string; blurb: string }> = {
  buy: { label: "Buying a key", blurb: "Get a Claude API key, pay your way, start in minutes." },
  free: { label: "Free & low cost", blurb: "Try every Claude model before you spend a cent." },
  integrate: { label: "Tool setup", blurb: "Wire the API into Cursor, VS Code, Claude Code and SDKs." },
  compare: { label: "Compare", blurb: "How apiToken.sale stacks up against the alternatives." },
  explain: { label: "How it works", blurb: "Pricing, billing, activation and support, explained." },
};

const BASE = "https://api.apitoken.sale";
const KEY = "sk-pool-•••";

// Shared reusable blocks -----------------------------------------------------
const quickSetupSteps: LearnBlock = {
  type: "steps",
  items: [
    "Create a free account and open the dashboard — no approval or waitlist.",
    "Generate one API key (it looks like sk-pool-…). The same key works across every supported Claude model.",
    `Point any Anthropic-compatible tool at ${BASE} and send requests to /v1/messages with the x-api-key header.`,
  ],
};

const cta = (): LearnBlock => ({ type: "note", text: "New accounts start with $10 of Claude usage at official API prices — enough to wire up your tools and run real calls before you top up." });

export const learnArticles: LearnArticle[] = [
  // ─────────────────────────── BUY ───────────────────────────
  {
    slug: "how-to-buy-claude-api-key",
    cluster: "buy",
    title: "How to Buy a Claude API Key",
    h1: "How to buy a Claude API key",
    description: "Buy a Claude API key in minutes with apiToken.sale — one key for every Claude model, prepaid balance, card or crypto payment, no Anthropic account required.",
    keywords: ["buy claude api key", "how to buy claude api", "claude api key", "purchase claude api access", "anthropic api key"],
    dek: "You do not need an Anthropic account, an invite, or a company card to start using Claude. With apiToken.sale you buy prepaid balance, generate one key, and call the same Anthropic Messages API at a discount.",
    sections: [
      { h2: "Get your key in three steps", blocks: [quickSetupSteps, cta()] },
      { h2: "How payment works", blocks: [
        { type: "p", text: "Top up any whole-dollar amount you like — there is no fixed product catalog. Your balance is prepaid, never expires, and is spent only when API requests run." },
        { type: "list", items: [
          "Pay by bank card or with cryptocurrency through a secure checkout provider.",
          "Every request is converted to official Anthropic API spend, then your active discount is applied.",
          "B2C accounts start 60% below official spend and progress up to 80% off as you top up more.",
        ] },
      ] },
      { h2: "What you can do with the key", blocks: [
        { type: "p", text: "One key unlocks the full supported Claude line — Opus, Sonnet and Haiku — across Claude Code, Cursor, Cline, Continue, Zed and the official Anthropic SDKs. Nothing about the protocol changes; only the price does." },
      ] },
    ],
    faq: [
      { q: "Do I need an Anthropic account to buy a Claude API key?", a: "No. apiToken.sale issues its own key and balance, so you can start without an Anthropic account, invite, or approval." },
      { q: "How fast is the key active?", a: "Instantly. You generate the key in the dashboard and it works on the next request — there is no waitlist or manual review." },
      { q: "How much does it cost to start?", a: "You can top up any whole-dollar amount, and every new account also gets $10 of Claude usage at official API prices for free." },
    ],
    related: ["claude-api-quick-setup", "cheapest-claude-api", "claude-api-crypto-payment", "free-claude-api-key"],
  },
  {
    slug: "cheapest-claude-api",
    cluster: "buy",
    title: "Cheapest Claude API Access — Up to 80% Off",
    h1: "The cheapest way to use the Claude API",
    description: "Cut Claude API costs by up to 80%. apiToken.sale sells the exact same Anthropic Messages API at a prepaid discount — same models, same endpoints, lower price per token.",
    keywords: ["cheapest claude api", "claude api discount", "cheap claude api", "claude api price", "save on anthropic api", "claude api cheaper than anthropic"],
    dek: "The Claude API is priced per token, and those tokens add up fast on long coding sessions. apiToken.sale gives you the identical API for up to 80% less by pooling prepaid balance and applying a progressive discount.",
    sections: [
      { h2: "Why it is cheaper", blocks: [
        { type: "p", text: "You send the same request to the same Anthropic Messages API and get the same response. The only thing under the hood is billing: each call is metered at official rates, then your discount is subtracted before it touches your balance." },
        { type: "list", items: [
          "B2C accounts begin at 60% off official spend.",
          "The discount grows to 80% off as your cumulative top-ups increase.",
          "B2B volume pricing is negotiated separately.",
        ] },
      ] },
      { h2: "Where the savings show up most", blocks: [
        { type: "p", text: "Agentic coding, long multi-turn sessions and prompt-cache-heavy workflows burn the most tokens — so they see the biggest absolute savings. Choosing the right model for each task compounds it further." },
        { type: "note", text: "Tip: route quick, cheap work to Haiku and reserve Opus for hard reasoning to stretch your balance further." },
      ] },
      { h2: "No subscription, no lock-in", blocks: [
        { type: "p", text: "There is no monthly fee. You top up prepaid balance that never expires and spend it only when requests run, so idle days cost nothing." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is this really the same Claude API?", a: "Yes — the same Anthropic Messages API, same model IDs, same request and response format. Only the price per call is lower." },
      { q: "How much can I save?", a: "B2C pricing starts at 60% below official API spend and progresses to as much as 80% off with higher cumulative top-ups." },
      { q: "Are there hidden fees or subscriptions?", a: "No. Balance is prepaid, never expires, and is consumed only by real API usage — there is no monthly charge." },
    ],
    related: ["claude-api-pricing-explained", "save-tokens-on-claude-api", "apitoken-vs-anthropic-direct", "how-billing-works"],
  },
  {
    slug: "claude-api-for-russia",
    cluster: "buy",
    title: "Claude API from Russia and Restricted Regions",
    h1: "Using the Claude API from Russia",
    description: "Access the Claude API from Russia and other restricted regions with apiToken.sale — no Anthropic account, pay by card or crypto, one key for every Claude model.",
    keywords: ["claude api russia", "claude api из россии", "anthropic api russia", "claude api restricted regions", "оплата claude api", "claude api без vpn"],
    dek: "Anthropic does not sell directly in every country, which leaves developers in Russia and other regions without an obvious way to pay. apiToken.sale removes that barrier: you buy prepaid balance and get a working key regardless of where Anthropic bills.",
    sections: [
      { h2: "Why direct access is hard", blocks: [
        { type: "p", text: "Signing up with Anthropic often requires a supported billing country and payment method. If you cannot complete that, you cannot get a key — even though the models themselves are reachable over the network." },
      ] },
      { h2: "How apiToken.sale solves it", blocks: [
        { type: "list", items: [
          "No Anthropic account needed — we issue the key and balance.",
          "Pay by bank card or cryptocurrency, whichever works for you.",
          "Instant activation, no waitlist, no company verification.",
        ] },
        cta(),
      ] },
      { h2: "Works with your existing tools", blocks: [
        { type: "p", text: `Point Claude Code, Cursor, Cline or the Anthropic SDK at ${BASE} and keep working exactly as before. Support is available in Russian and English over Telegram.` },
      ] },
    ],
    faq: [
      { q: "Can I pay from Russia?", a: "Yes. You can pay by bank card or with cryptocurrency through a checkout provider, so a supported Anthropic billing country is not required." },
      { q: "Do I need a VPN?", a: "You do not need an Anthropic account or billing country. Network reachability depends on your own connection, but there is no geographic gate on issuing your key and balance." },
      { q: "Is support available in Russian?", a: "Yes — support is available in Russian and English through Telegram." },
    ],
    related: ["claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
  },
  {
    slug: "claude-api-crypto-payment",
    cluster: "buy",
    title: "Pay for the Claude API with Crypto",
    h1: "Pay for the Claude API with cryptocurrency",
    description: "Buy Claude API balance with cryptocurrency or bank card on apiToken.sale. No Anthropic account, instant activation, prepaid balance that never expires.",
    keywords: ["claude api crypto payment", "buy claude api with crypto", "claude api usdt", "pay anthropic api crypto", "claude api bitcoin"],
    dek: "If a bank card is not an option — or you simply prefer crypto — you can fund your Claude API balance with cryptocurrency and start immediately.",
    sections: [
      { h2: "Card or crypto, your choice", blocks: [
        { type: "p", text: "At checkout you can pay by bank card or with cryptocurrency through a secure payment provider. Either way the balance lands prepaid in your account and is spent only when requests run." },
      ] },
      { h2: "Why crypto helps", blocks: [
        { type: "list", items: [
          "No supported Anthropic billing country required.",
          "Useful where cards are declined or unavailable.",
          "Balance never expires, so you fund once and draw down as you build.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which payment methods are supported?", a: "You can pay by bank card or with cryptocurrency through a checkout provider." },
      { q: "Does the balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
    ],
    related: ["claude-api-for-russia", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-refund-policy"],
  },
  {
    slug: "claude-api-without-waitlist",
    cluster: "buy",
    title: "Claude API With No Waitlist or Approval",
    h1: "Claude API access with no waitlist",
    description: "Skip the Anthropic waitlist and approval. Create an account on apiToken.sale, generate a Claude API key, and make your first call in minutes.",
    keywords: ["claude api no waitlist", "claude api instant access", "claude api without approval", "get claude api key fast", "claude api no anthropic account"],
    dek: "Waiting for approval kills momentum. apiToken.sale gives you instant, self-serve access to every supported Claude model — no queue, no sales call, no company verification.",
    sections: [
      { h2: "Instant, self-serve access", blocks: [ quickSetupSteps, cta() ] },
      { h2: "What 'instant' actually means", blocks: [
        { type: "p", text: "The moment you generate a key it is live. There is no manual review step between signing up and your first successful request, so you can wire up a tool and ship in the same sitting." },
      ] },
    ],
    faq: [
      { q: "Is there really no waitlist?", a: "Correct. Access is self-serve and instant — you generate a key and it works on the next request." },
      { q: "Do I need to talk to sales?", a: "No. B2C access is fully self-serve. Only negotiated B2B volume pricing involves a conversation." },
    ],
    related: ["how-to-buy-claude-api-key", "claude-api-quick-setup", "claude-api-activation-time", "free-claude-api-key"],
  },
  {
    slug: "claude-api-quick-setup",
    cluster: "buy",
    title: "Claude API Setup in Two Minutes",
    h1: "Set up the Claude API in two minutes",
    description: "A two-minute Claude API quickstart: create a key, set your base URL to api.apitoken.sale, and send your first /v1/messages request with curl, Python or your IDE.",
    keywords: ["claude api quickstart", "claude api setup", "claude api first request", "anthropic messages api", "claude api base url"],
    dek: "This is the fastest path from zero to a working Claude API call. Everything below uses the standard Anthropic Messages API, so it drops straight into your existing code.",
    sections: [
      { h2: "1. Create a key", blocks: [ { type: "p", text: "Sign up, open the dashboard, and generate a key. It looks like sk-pool-… and works across every supported model." } ] },
      { h2: "2. Set your endpoint", blocks: [
        { type: "p", text: "Point any Anthropic-compatible client at the gateway:" },
        { type: "code", code: `Base URL:  ${BASE}\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: ${KEY}\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. Send your first request", blocks: [
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        cta(),
      ] },
    ],
    faq: [
      { q: "What base URL do I use?", a: `Use ${BASE} with any Anthropic-compatible tool and send requests to /v1/messages.` },
      { q: "Which auth header is required?", a: "Send x-api-key with your key and anthropic-version, exactly like the official Anthropic API." },
    ],
    related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "how-to-buy-claude-api-key", "claude-api-for-vs-code"],
  },

  // ─────────────────────────── FREE ───────────────────────────
  {
    slug: "free-claude-api-key",
    cluster: "free",
    title: "Free Claude API Key to Get Started",
    h1: "Get a free Claude API key to start",
    description: "Create a Claude API key for free on apiToken.sale and get $10 of Claude usage at official API prices — no card required, no Anthropic account, instant access.",
    keywords: ["free claude api key", "claude api free", "claude api free tier", "free anthropic api key", "claude api no card"],
    dek: "You can create your key and make real Claude calls before spending anything. Every new B2C account is funded with $10 of usage at official API prices so you can prove the integration works first.",
    sections: [
      { h2: "What 'free' includes", blocks: [
        { type: "list", items: [
          "A working API key across all supported Claude models.",
          "$10 of Claude usage at official API prices, no card required.",
          "Enough headroom to wire up your tools and run genuine requests.",
        ] },
        { type: "p", text: "When you are ready for more, top up any whole-dollar amount and your discount kicks in automatically." },
      ] },
      { h2: "How to claim it", blocks: [ quickSetupSteps ] },
    ],
    faq: [
      { q: "Is the free usage real API access?", a: "Yes. The $10 of included usage runs against the same Claude models and endpoints as paid balance." },
      { q: "Do I need a card to start?", a: "No card is required to create your account and use the included $10 of usage." },
    ],
    related: ["claude-api-free-trial", "how-to-buy-claude-api-key", "claude-code-without-subscription", "cheapest-claude-api"],
  },
  {
    slug: "claude-api-free-trial",
    cluster: "free",
    title: "Claude API Free Trial — Start in Minutes",
    h1: "Try the Claude API free",
    description: "Start coding with Claude in minutes. apiToken.sale gives every new account $10 of Claude usage at official prices, no card and no Anthropic approval required.",
    keywords: ["claude api free trial", "try claude api", "claude api test", "claude api sandbox", "claude api demo"],
    dek: "There is no separate trial to apply for — you simply sign up, get $10 of usage at official API prices, and run real calls against every supported model.",
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
      { q: "How do I start the trial?", a: "Just create an account — the $10 of official-price usage is added automatically, with no application step." },
      { q: "What happens when the free usage runs out?", a: "Top up any whole-dollar amount to keep going; your progressive discount applies immediately." },
    ],
    related: ["free-claude-api-key", "claude-api-without-waitlist", "claude-api-quick-setup", "claude-haiku-api"],
  },
  {
    slug: "claude-code-without-subscription",
    cluster: "free",
    title: "Use Claude Code Without a Subscription",
    h1: "Claude Code without the $200/month plan",
    description: "Run Claude Code on pay-as-you-go API balance instead of a monthly subscription. Set ANTHROPIC_BASE_URL to api.apitoken.sale and pay only for what you use.",
    keywords: ["claude code without subscription", "claude code api key", "claude code pay as you go", "claude code cheap", "claude code no subscription"],
    dek: "Claude Code does not have to mean a fixed monthly plan. Point it at an API key with prepaid balance and you pay per token — ideal if your usage is spiky or you just want to try it.",
    sections: [
      { h2: "Two environment variables", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
        { type: "p", text: "That is the entire change. Claude Code keeps every feature — it simply bills against your prepaid balance at a discount instead of a subscription." },
      ] },
      { h2: "When pay-as-you-go wins", blocks: [
        { type: "list", items: [
          "Occasional or bursty usage where a flat monthly fee is wasteful.",
          "Trying Claude Code before committing to a plan.",
          "Keeping several tools on one balance and one key.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Does Claude Code work with a custom API key?", a: "Yes. Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY and Claude Code uses your key and balance directly." },
      { q: "Do I lose any features?", a: "No. Claude Code behaves identically; only billing changes from a subscription to prepaid per-token usage." },
    ],
    related: ["claude-api-key-for-cursor", "cheapest-claude-api", "claude-opus-api", "anthropic-sdk-base-url"],
  },
  {
    slug: "claude-opus-api",
    cluster: "free",
    title: "Claude Opus API Access",
    h1: "Claude Opus 4.8 through the API",
    description: "Access Claude Opus 4.8 and 4.7 through one apiToken.sale key at up to 80% off official rates. Best for complex reasoning, refactors and long agent sessions.",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api key", "claude opus pricing", "claude opus discount"],
    dek: "Opus is Claude's most capable tier — the model to reach for on hard reasoning, architecture and long agentic runs. apiToken.sale gives you Opus 4.8 and 4.7 on the same key and balance as every other model.",
    sections: [
      { h2: "When to use Opus", blocks: [
        { type: "list", items: [
          "Complex refactors and multi-file changes.",
          "Architecture, planning and high-stakes reasoning.",
          "Long sessions where consistency and cache reuse matter.",
        ] },
      ] },
      { h2: "Opus on your balance", blocks: [
        { type: "p", text: "Opus 4.8 (model ID claude-opus-4-8) and Opus 4.7 are billed at official token rates minus your discount, so you get the top tier for a fraction of the list price." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which Opus models are available?", a: "Claude Opus 4.8 (claude-opus-4-8) and Claude Opus 4.7, on the same key and prepaid balance as Sonnet and Haiku." },
      { q: "Is Opus worth the extra tokens?", a: "For complex reasoning, refactors and long agent runs, yes. For fast, cheap tasks, Haiku or Sonnet is usually the better value." },
    ],
    related: ["claude-opus-vs-sonnet", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  },
  {
    slug: "claude-sonnet-api",
    cluster: "free",
    title: "Claude Sonnet API Access",
    h1: "Claude Sonnet through the API",
    description: "Use Claude Sonnet 5 and Sonnet 4.6 through apiToken.sale — the default model for daily coding and agents, at up to 80% off official API pricing.",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api key", "claude sonnet pricing", "best claude model for coding"],
    dek: "Sonnet is the workhorse: fast enough for interactive coding, smart enough for real agent workflows. apiToken.sale serves Sonnet 5 and Sonnet 4.6 on one discounted balance.",
    sections: [
      { h2: "The daily-driver model", blocks: [
        { type: "p", text: "For most coding and agent tasks, Sonnet is the right default — a strong balance of quality, speed and cost. Reserve Opus for the genuinely hard problems." },
      ] },
      { h2: "Sonnet pricing note", blocks: [
        { type: "p", text: "Claude Sonnet 5 (claude-sonnet-5) ships with introductory official rates, and the engine always applies the current effective rate before your discount. Sonnet 4.6 remains available on the same key." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which Sonnet models can I use?", a: "Claude Sonnet 5 (claude-sonnet-5) and Claude Sonnet 4.6, on the same balance as Opus and Haiku." },
      { q: "Is Sonnet good for coding?", a: "Yes — Sonnet is the recommended default for everyday coding and agent workflows." },
    ],
    related: ["claude-opus-vs-sonnet", "claude-opus-api", "claude-haiku-api", "claude-api-key-for-cursor"],
  },
  {
    slug: "claude-haiku-api",
    cluster: "free",
    title: "Claude Haiku API Access",
    h1: "Claude Haiku 4.5 through the API",
    description: "Access Claude Haiku 4.5 through apiToken.sale — the fastest, most economical Claude model, ideal for high-volume and low-latency tasks, at a prepaid discount.",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "fastest claude model", "cheap claude model", "haiku api key"],
    dek: "Haiku is built for speed and volume: classification, extraction, routing and any task where latency and cost matter more than deep reasoning.",
    sections: [
      { h2: "When Haiku is the right call", blocks: [
        { type: "list", items: [
          "High-volume, low-latency requests.",
          "Cheap background tasks and pre-processing.",
          "Stretching your balance on work that does not need Opus.",
        ] },
        cta(),
      ] },
      { h2: "Mix models on one key", blocks: [
        { type: "p", text: "Because every model shares one key and balance, you can route cheap work to Haiku (claude-haiku-4-5) and escalate only the hard requests to Sonnet or Opus." },
      ] },
    ],
    faq: [
      { q: "How fast and cheap is Haiku?", a: "Haiku 4.5 is the fastest and lowest-cost Claude model, ideal for high-volume, latency-sensitive work." },
      { q: "Can I combine Haiku with other models?", a: "Yes. One key and balance covers Haiku, Sonnet and Opus, so you can route each task to the best-value model." },
    ],
    related: ["claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api", "cheapest-claude-api"],
  },

  // ─────────────────────────── INTEGRATE ───────────────────────────
  {
    slug: "claude-api-key-for-cursor",
    cluster: "integrate",
    title: "Claude API Key for Cursor",
    h1: "Use a Claude API key in Cursor",
    description: "Connect Cursor to Claude with an apiToken.sale key: set the Anthropic base URL to api.apitoken.sale, paste your key, pick a model, and code at up to 80% off.",
    keywords: ["claude api key for cursor", "cursor claude api", "cursor anthropic key", "use claude in cursor", "cursor without cursor pro"],
    dek: "Cursor lets you bring your own Anthropic key, which means you can run Claude in Cursor on discounted prepaid balance instead of a bundled plan.",
    sections: [
      { h2: "Three-step setup", blocks: [
        { type: "steps", items: [
          "Open Cursor → Settings → Models → Anthropic API.",
          `Set the base URL to ${BASE} and paste your ${KEY} key.`,
          "Pick a model such as claude-opus-4-8 and start coding.",
        ] },
      ] },
      { h2: "Configuration", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
        cta(),
      ] },
    ],
    faq: [
      { q: "Can I use my own Claude key in Cursor?", a: "Yes. Cursor's Anthropic provider accepts a custom base URL and key, so you can point it at apiToken.sale." },
      { q: "Do I still need Cursor Pro?", a: "You can run Claude through your own API key and balance; features that require Cursor's own plan are separate from the model provider." },
    ],
    related: ["cursor-without-anthropic-account", "claude-api-for-vs-code", "claude-api-quick-setup", "claude-sonnet-api"],
  },
  {
    slug: "claude-api-for-vs-code",
    cluster: "integrate",
    title: "Claude API in VS Code (Cline, Continue)",
    h1: "Use the Claude API in VS Code",
    description: "Run Claude in VS Code with Cline or Continue using an apiToken.sale key. Set the Anthropic base URL to api.apitoken.sale and pay per token at a discount.",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "claude in vscode", "vscode anthropic api key"],
    dek: "Free VS Code agents like Cline and Continue accept any Anthropic-compatible endpoint, so you can code with Claude inside VS Code on discounted balance.",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "${BASE}",\n    "apiKey": "${KEY}",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which VS Code extensions work?", a: "Any extension that supports an Anthropic-compatible endpoint, including Cline and Continue, works with an apiToken.sale key." },
      { q: "Do I need a paid extension?", a: "No. Cline and Continue are free; you only pay for the Claude API usage against your prepaid balance." },
    ],
    related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "claude-api-quick-setup", "claude-code-without-subscription"],
  },
  {
    slug: "cursor-without-anthropic-account",
    cluster: "integrate",
    title: "Claude in Cursor Without an Anthropic Account",
    h1: "Run Claude in Cursor without an Anthropic account",
    description: "No Anthropic account? Use Claude in Cursor with an apiToken.sale key instead. Instant access, card or crypto payment, and up to 80% off official API rates.",
    keywords: ["cursor without anthropic account", "claude cursor no anthropic", "cursor claude api key", "use claude without anthropic account"],
    dek: "If you cannot or would rather not create an Anthropic account, apiToken.sale issues its own key that Cursor accepts as an Anthropic provider.",
    sections: [
      { h2: "Why this works", blocks: [
        { type: "p", text: "Cursor talks to the Anthropic Messages API. apiToken.sale exposes exactly that API, so Cursor cannot tell the difference — it just uses your key and base URL." },
      ] },
      { h2: "Set it up", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
        cta(),
      ] },
    ],
    faq: [
      { q: "Do I need an Anthropic account for this?", a: "No. apiToken.sale provides the key and balance, so no Anthropic account is required." },
      { q: "Is the integration official Anthropic API?", a: "Cursor uses the standard Anthropic Messages API; apiToken.sale serves that same API at a discount." },
    ],
    related: ["claude-api-key-for-cursor", "claude-api-for-russia", "how-to-buy-claude-api-key", "apitoken-vs-anthropic-direct"],
  },
  {
    slug: "anthropic-sdk-base-url",
    cluster: "integrate",
    title: "Use Anthropic SDKs with a Custom Base URL",
    h1: "Point the Anthropic SDK at apiToken.sale",
    description: "Use the official Anthropic Python and TypeScript SDKs with apiToken.sale by setting base_url to api.apitoken.sale. Same SDK, same code, lower cost per token.",
    keywords: ["anthropic sdk base url", "anthropic python sdk custom endpoint", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
    dek: "The official Anthropic SDKs let you override the base URL, so switching to apiToken.sale is a one-line change — your model IDs and message code stay exactly the same.",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        cta(),
      ] },
    ],
    faq: [
      { q: "Can I keep using the official Anthropic SDK?", a: "Yes. Set base_url (Python) or baseURL (TypeScript) to apiToken.sale and everything else stays the same." },
      { q: "Do model IDs change?", a: "No. Use the same model IDs such as claude-opus-4-8 and claude-sonnet-5." },
    ],
    related: ["claude-api-quick-setup", "claude-api-for-vs-code", "claude-code-without-subscription", "claude-api-key-for-cursor"],
  },

  // ─────────────────────────── COMPARE ───────────────────────────
  {
    slug: "apitoken-vs-anthropic-direct",
    cluster: "compare",
    title: "apiToken.sale vs Anthropic Direct",
    h1: "apiToken.sale vs buying from Anthropic directly",
    description: "Compare apiToken.sale and Anthropic direct: identical Messages API and models, but with up to 80% off, no account requirement, and card or crypto payment.",
    keywords: ["claude api vs anthropic direct", "apitoken vs anthropic", "anthropic api alternative", "cheaper than anthropic api", "claude api reseller"],
    dek: "apiToken.sale is not a different API — it is the same Anthropic Messages API, resold from prepaid balance at a discount. Here is what actually changes and what does not.",
    sections: [
      { h2: "What stays the same", blocks: [
        { type: "list", items: [
          "The same Anthropic Messages API, endpoints and streaming.",
          "The same model IDs (claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5).",
          "The same request and response format your code already expects.",
        ] },
      ] },
      { h2: "What changes", blocks: [
        { type: "list", items: [
          "Price: up to 80% below official spend for B2C.",
          "Onboarding: no Anthropic account, waitlist or billing-country requirement.",
          "Payment: bank card or cryptocurrency.",
        ] },
        cta(),
      ] },
      { h2: "Who each is for", blocks: [
        { type: "p", text: "If you already have frictionless Anthropic billing and enterprise agreements, direct may suit you. If you want the same models cheaper, faster to start, and payable by card or crypto, apiToken.sale is the pragmatic choice." },
      ] },
    ],
    faq: [
      { q: "Is apiToken.sale the real Claude API?", a: "Yes — it serves the same Anthropic Messages API and models. Only pricing and onboarding differ." },
      { q: "Why is it cheaper than Anthropic direct?", a: "Balance is prepaid and pooled, and a progressive discount of up to 80% is applied to official spend." },
    ],
    related: ["cheapest-claude-api", "apitoken-vs-openrouter", "claude-api-pricing-explained", "how-billing-works"],
  },
  {
    slug: "apitoken-vs-openrouter",
    cluster: "compare",
    title: "apiToken.sale vs OpenRouter for Claude",
    h1: "apiToken.sale vs OpenRouter for Claude",
    description: "Choosing a Claude gateway? Compare apiToken.sale and OpenRouter: a native Anthropic endpoint and prepaid discount vs a multi-provider router.",
    keywords: ["openrouter alternative", "apitoken vs openrouter", "claude api gateway", "openrouter claude", "best claude api gateway"],
    dek: "Both let you reach Claude without an Anthropic account, but they are built differently. If Claude is your main model, a native Anthropic endpoint keeps things simple.",
    sections: [
      { h2: "Native Anthropic endpoint", blocks: [
        { type: "p", text: `apiToken.sale exposes the standard Anthropic Messages API at ${BASE}, so Claude Code, Cursor and the Anthropic SDKs work with zero adapters. You are not routing through a generic multi-provider abstraction.` },
      ] },
      { h2: "Prepaid discount, not markup", blocks: [
        { type: "list", items: [
          "Progressive B2C discount up to 80% off official Claude spend.",
          "One key and balance for Opus, Sonnet and Haiku.",
          "Card or crypto top-ups that never expire.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Why pick a Claude-native gateway?", a: "If Claude is your primary model, a native Anthropic endpoint means your existing Anthropic tools and SDKs work unchanged." },
      { q: "Does apiToken.sale mark prices up?", a: "No — it applies a discount to official Claude spend rather than adding a markup." },
    ],
    related: ["apitoken-vs-anthropic-direct", "cheapest-claude-api", "claude-api-quick-setup", "anthropic-sdk-base-url"],
  },
  {
    slug: "claude-opus-vs-sonnet",
    cluster: "compare",
    title: "Claude Opus vs Sonnet — Which to Use",
    h1: "Claude Opus vs Sonnet: which model to use",
    description: "Opus or Sonnet? A practical guide to picking the right Claude model for coding and agents — and using both on one apiToken.sale key and balance.",
    keywords: ["claude opus vs sonnet", "which claude model", "opus or sonnet coding", "best claude model", "claude model comparison"],
    dek: "Opus and Sonnet solve different problems. Choosing well is the easiest way to get better results and spend fewer tokens — and you can keep both on one key.",
    sections: [
      { h2: "Use Sonnet by default", blocks: [
        { type: "p", text: "Sonnet 5 and Sonnet 4.6 handle the vast majority of coding and agent work quickly and cost-effectively. Start here." },
      ] },
      { h2: "Escalate to Opus for hard problems", blocks: [
        { type: "p", text: "Reach for Opus 4.8 on complex refactors, architecture, and long high-stakes sessions where extra reasoning pays for itself." },
        { type: "note", text: "Because one key covers both, you can route each task to the right tier without juggling providers." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Which is better for coding?", a: "Sonnet is the recommended default for daily coding; use Opus for complex reasoning and long refactors." },
      { q: "Can I use both on one account?", a: "Yes. Opus, Sonnet and Haiku all share the same key and prepaid balance." },
    ],
    related: ["claude-opus-api", "claude-sonnet-api", "claude-haiku-api", "save-tokens-on-claude-api"],
  },

  // ─────────────────────────── EXPLAIN ───────────────────────────
  {
    slug: "claude-api-pricing-explained",
    cluster: "explain",
    title: "Claude API Pricing Explained",
    h1: "How Claude API pricing works",
    description: "Understand Claude API pricing: per-token input and output rates, prompt caching, and how apiToken.sale applies a progressive discount of up to 80% off.",
    keywords: ["claude api pricing", "claude api cost", "how claude api pricing works", "claude token pricing", "anthropic api pricing explained"],
    dek: "Claude is billed per token — separately for input and output — with discounts for cached content. apiToken.sale keeps those mechanics identical and layers a discount on top.",
    sections: [
      { h2: "Tokens, input and output", blocks: [
        { type: "p", text: "Every request is metered by tokens in (your prompt and context) and tokens out (the model's reply). Output tokens usually cost more than input, and larger models cost more per token." },
      ] },
      { h2: "Caching and thinking", blocks: [
        { type: "list", items: [
          "Cache writes and cache reads are metered separately, and cache reads are much cheaper.",
          "Thinking tokens count toward output on reasoning-heavy calls.",
          "Streaming and non-streaming requests are billed the same way.",
        ] },
      ] },
      { h2: "The apiToken.sale discount", blocks: [
        { type: "p", text: "Each call is converted to official Anthropic spend, then your discount is subtracted: B2C starts at 60% off and progresses to 80% off as cumulative top-ups grow. Every request is visible in your dashboard with token-level detail." },
        cta(),
      ] },
    ],
    faq: [
      { q: "How is the Claude API priced?", a: "Per token, split into input and output, with separate cheaper rates for cache reads. Larger models cost more per token." },
      { q: "How does the discount apply?", a: "Official spend is calculated first, then your B2C discount (60% up to 80%) is subtracted before it touches your balance." },
    ],
    related: ["cheapest-claude-api", "save-tokens-on-claude-api", "how-billing-works", "apitoken-vs-anthropic-direct"],
  },
  {
    slug: "save-tokens-on-claude-api",
    cluster: "explain",
    title: "How to Save Tokens on the Claude API",
    h1: "How to save tokens on the Claude API",
    description: "Cut Claude API costs with prompt caching, the right model per task, and tighter context. Practical token-saving tactics that stack with the apiToken.sale discount.",
    keywords: ["save tokens claude api", "reduce claude api cost", "claude prompt caching", "claude api optimization", "lower claude api bill"],
    dek: "Your discount lowers the price per token; these tactics lower the number of tokens. Together they compound into a much smaller bill.",
    sections: [
      { h2: "Use prompt caching", blocks: [
        { type: "p", text: "Long, stable context — system prompts, large files, tool definitions — should be cached. Cache reads cost a fraction of fresh input tokens, so repeated context becomes cheap." },
      ] },
      { h2: "Pick the right model", blocks: [
        { type: "p", text: "Do not send every request to Opus. Route cheap or high-volume work to Haiku, keep everyday coding on Sonnet, and reserve Opus for genuinely hard reasoning." },
      ] },
      { h2: "Trim context", blocks: [
        { type: "list", items: [
          "Send only the files and history a task actually needs.",
          "Summarize long threads instead of resending them in full.",
          "Cap max_tokens to what the response really requires.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "What is the single biggest token saver?", a: "Prompt caching for large, repeated context, combined with choosing the cheapest model that can do the job." },
      { q: "Do these tips stack with the discount?", a: "Yes. The discount lowers price per token; these tactics lower token count, so the savings multiply." },
    ],
    related: ["claude-api-pricing-explained", "cheapest-claude-api", "claude-haiku-api", "claude-opus-vs-sonnet"],
  },
  {
    slug: "how-billing-works",
    cluster: "explain",
    title: "How Billing Works on apiToken.sale",
    h1: "How billing works",
    description: "Understand apiToken.sale billing: prepaid balance, per-request metering at official rates, your progressive discount, and token-level usage in the dashboard.",
    keywords: ["claude api billing", "how apitoken billing works", "prepaid claude api", "claude api usage tracking", "claude api balance"],
    dek: "Billing is prepaid and transparent. You fund a balance, and each request draws down official spend minus your discount, with a full breakdown you can audit.",
    sections: [
      { h2: "Prepaid balance", blocks: [
        { type: "p", text: "You top up any whole-dollar amount. Balance never expires and there is no subscription, so idle time costs nothing." },
      ] },
      { h2: "Per-request metering", blocks: [
        { type: "list", items: [
          "Each call is converted to official Anthropic spend by token.",
          "Your active discount (60% up to 80% for B2C) is subtracted.",
          "The net amount is deducted from your prepaid balance.",
        ] },
      ] },
      { h2: "Full visibility", blocks: [
        { type: "p", text: "Every request appears in your dashboard with input, output, cache and thinking tokens, so you always know where your balance goes." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is billing prepaid or postpaid?", a: "Prepaid. You fund a balance in advance and requests draw it down; there is no monthly invoice." },
      { q: "Can I see token-level usage?", a: "Yes. The dashboard breaks every request down by input, output, cache read/write and thinking tokens." },
    ],
    related: ["claude-api-pricing-explained", "claude-api-refund-policy", "claude-api-activation-time", "cheapest-claude-api"],
  },
  {
    slug: "claude-api-activation-time",
    cluster: "explain",
    title: "How Fast Is Claude API Activation?",
    h1: "How fast your Claude API key activates",
    description: "apiToken.sale keys activate instantly. Generate a key, top up, and make a successful Claude API call within minutes — no manual review or waitlist.",
    keywords: ["claude api activation time", "how fast claude api key", "instant claude api key", "claude api ready time"],
    dek: "There is no waiting period between creating your key and using it. Activation is instant, so the only limit on speed is how fast you paste the key into your tool.",
    sections: [
      { h2: "Instant by design", blocks: [
        { type: "p", text: "Keys are live the moment you generate them. Top-ups credit your balance as soon as payment confirms, and card payments confirm in seconds." },
        cta(),
      ] },
    ],
    faq: [
      { q: "How long until my key works?", a: "Immediately. There is no manual review — a freshly generated key works on the next request." },
      { q: "How long do top-ups take?", a: "Card payments credit in seconds; crypto credits after the network confirms the transaction." },
    ],
    related: ["claude-api-without-waitlist", "how-to-buy-claude-api-key", "how-billing-works", "claude-api-quick-setup"],
  },
  {
    slug: "claude-api-supported-countries",
    cluster: "explain",
    title: "Claude API Supported Countries",
    h1: "Where you can use apiToken.sale",
    description: "apiToken.sale works worldwide with no Anthropic billing-country requirement. Pay by card or crypto and use the Claude API from regions Anthropic does not serve directly.",
    keywords: ["claude api supported countries", "claude api worldwide", "anthropic api country restrictions", "claude api available regions"],
    dek: "Because we issue your key and balance, there is no Anthropic billing-country gate. That opens the Claude API to developers in regions where signing up directly is difficult.",
    sections: [
      { h2: "No billing-country gate", blocks: [
        { type: "list", items: [
          "No Anthropic account or supported billing country required.",
          "Card and cryptocurrency payment options.",
          "Support in English and Russian over Telegram.",
        ] },
        cta(),
      ] },
    ],
    faq: [
      { q: "Is the Claude API available in my country?", a: "apiToken.sale has no billing-country requirement, so you can buy balance and use a key from regions Anthropic does not bill directly." },
      { q: "What about payment restrictions?", a: "You can pay by card or with cryptocurrency, which helps where cards are unavailable." },
    ],
    related: ["claude-api-for-russia", "claude-api-crypto-payment", "how-to-buy-claude-api-key", "claude-api-without-waitlist"],
  },
  {
    slug: "claude-api-refund-policy",
    cluster: "explain",
    title: "Claude API Refund Policy",
    h1: "Refunds and support",
    description: "Learn how apiToken.sale handles balance, refunds and support. Prepaid balance never expires, and help is available in English and Russian through Telegram.",
    keywords: ["claude api refund", "apitoken refund policy", "claude api support", "claude api money back", "claude api help"],
    dek: "Prepaid balance is designed to be low-risk: it never expires, you spend only what you call, and support is one message away.",
    sections: [
      { h2: "Balance and refunds", blocks: [
        { type: "p", text: "Because balance is prepaid and never expires, unused funds stay available for future usage. Refund handling is processed through the original payment provider; reach out to support with your account details." },
      ] },
      { h2: "Getting help", blocks: [
        { type: "p", text: "Support is available in English and Russian via Telegram, and by email at apitokensale@gmail.com. Most integration questions are answered quickly." },
        cta(),
      ] },
    ],
    faq: [
      { q: "Does my balance expire?", a: "No. Prepaid balance never expires and is consumed only by real API usage." },
      { q: "How do I contact support?", a: "Reach support in English or Russian through Telegram, or by email at apitokensale@gmail.com." },
    ],
    related: ["how-billing-works", "claude-api-crypto-payment", "claude-api-supported-countries", "how-to-buy-claude-api-key"],
  },
];

export const learnArticlesBySlug: Record<string, LearnArticle> = Object.fromEntries(
  learnArticles.map((article) => [article.slug, article]),
);

export function learnPath(slug: string): string {
  return `/docs/learn/${slug}`;
}

export const LEARN_HUB_PATH = "/docs/learn";

/** Path to the clean-markdown version of a learn article (AI-agent gateway). */
export function learnMarkdownPath(slug: string): string {
  return `/md/docs/learn/${slug}`;
}

function blockToMarkdown(block: LearnBlock): string {
  switch (block.type) {
    case "p":
      return block.text;
    case "note":
      return `> ${block.text}`;
    case "list":
      return block.items.map((item) => `- ${item}`).join("\n");
    case "steps":
      return block.items.map((item, index) => `${index + 1}. ${item}`).join("\n");
    case "code":
      return "```\n" + block.code + "\n```";
    default:
      return "";
  }
}

/** Serialize an article to clean Markdown for the AI-agent gateway. */
export function renderLearnMarkdown(article: LearnArticle, origin: string): string {
  const lines: string[] = [
    "---",
    `title: ${article.title}`,
    `description: ${JSON.stringify(article.description)}`,
    `url: ${origin}${learnPath(article.slug)}`,
    "language: en",
    "---",
    "",
    `# ${article.h1}`,
    "",
    article.dek,
    "",
  ];
  for (const section of article.sections) {
    lines.push(`## ${section.h2}`, "");
    for (const block of section.blocks) {
      lines.push(blockToMarkdown(block), "");
    }
  }
  if (article.faq.length > 0) {
    lines.push("## Frequently asked questions", "");
    for (const item of article.faq) {
      lines.push(`### ${item.q}`, "", item.a, "");
    }
  }
  lines.push("---", `Get a key: ${origin}/register`, `More guides: ${origin}${LEARN_HUB_PATH}`, "");
  return lines.join("\n");
}
