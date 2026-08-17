// Hand-written core learn articles, one module per article.
// Composed here in the original order from learn.ts.

import type { LearnArticle } from "../learn";
import { article as howToBuyClaudeApiKey } from "./how-to-buy-claude-api-key";
import { article as cheapestClaudeApi } from "./cheapest-claude-api";
import { article as claudeApiForRussia } from "./claude-api-for-russia";
import { article as claudeApiCryptoPayment } from "./claude-api-crypto-payment";
import { article as claudeApiWithoutWaitlist } from "./claude-api-without-waitlist";
import { article as claudeApiQuickSetup } from "./claude-api-quick-setup";
import { article as freeClaudeApiKey } from "./free-claude-api-key";
import { article as claudeApiFreeTrial } from "./claude-api-free-trial";
import { article as claudeCodeWithoutSubscription } from "./claude-code-without-subscription";
import { article as claudeOpusApi } from "./claude-opus-api";
import { article as claudeSonnetApi } from "./claude-sonnet-api";
import { article as claudeHaikuApi } from "./claude-haiku-api";
import { article as claudeApiKeyForCursor } from "./claude-api-key-for-cursor";
import { article as claudeApiForVsCode } from "./claude-api-for-vs-code";
import { article as cursorWithoutAnthropicAccount } from "./cursor-without-anthropic-account";
import { article as anthropicSdkBaseUrl } from "./anthropic-sdk-base-url";
import { article as claudeApiLangchain } from "./claude-api-langchain";
import { article as claudeApiLitellm } from "./claude-api-litellm";
import { article as claudeApiAider } from "./claude-api-aider";
import { article as claudeApiRooCode } from "./claude-api-roo-code";
import { article as apitokenVsAnthropicDirect } from "./apitoken-vs-anthropic-direct";
import { article as apitokenVsOpenrouter } from "./apitoken-vs-openrouter";
import { article as claudeOpusVsSonnet } from "./claude-opus-vs-sonnet";
import { article as claudeApiPricingExplained } from "./claude-api-pricing-explained";
import { article as saveTokensOnClaudeApi } from "./save-tokens-on-claude-api";
import { article as howBillingWorks } from "./how-billing-works";
import { article as claudeApiActivationTime } from "./claude-api-activation-time";
import { article as claudeApiSupportedCountries } from "./claude-api-supported-countries";
import { article as claudeApiRefundPolicy } from "./claude-api-refund-policy";
import { article as apitokenVsProxyapi } from "./apitoken-vs-proxyapi";
import { article as apitokenVsPortkey } from "./apitoken-vs-portkey";
import { article as apitokenVsLitellm } from "./apitoken-vs-litellm";
import { article as bestClaudeModelForCoding } from "./best-claude-model-for-coding";
import { article as claudeMaxPlanVsApi } from "./claude-max-plan-vs-api";
import { article as claude35VsClaude4 } from "./claude-3-5-vs-claude-4";
import { article as whyChooseApitoken } from "./why-choose-apitoken";
import { article as claudeApiGateway } from "./claude-api-gateway";
import { article as claudeApiRateLimits } from "./claude-api-rate-limits";
import { article as claudeApiStreaming } from "./claude-api-streaming";
import { article as claudeApiPromptCaching } from "./claude-api-prompt-caching";
import { article as claudeApiBestPractices } from "./claude-api-best-practices";
import { article as claudeCodeApiKey } from "./claude-code-api-key";
import { article as openaiApiQuickstart } from "./openai-api-quickstart";
import { article as codexCliSetup } from "./codex-cli-setup";
import { article as vscodeAiAgentsOnePrompt } from "./vscode-ai-agents-one-prompt";
import { article as claudeApiKeySecurity } from "./claude-api-key-security";
import { article as claudeApiForAiAgents } from "./claude-api-for-ai-agents";

export const coreLearnArticles: LearnArticle[] = [
  // ─────────────────────────── BUY ───────────────────────────
  howToBuyClaudeApiKey,
  cheapestClaudeApi,
  claudeApiForRussia,
  claudeApiCryptoPayment,
  claudeApiWithoutWaitlist,
  claudeApiQuickSetup,
  // ─────────────────────────── FREE ───────────────────────────
  freeClaudeApiKey,
  claudeApiFreeTrial,
  claudeCodeWithoutSubscription,
  claudeOpusApi,
  claudeSonnetApi,
  claudeHaikuApi,
  // ─────────────────────────── INTEGRATE ───────────────────────────
  claudeApiKeyForCursor,
  claudeApiForVsCode,
  cursorWithoutAnthropicAccount,
  anthropicSdkBaseUrl,
  claudeApiLangchain,
  claudeApiLitellm,
  claudeApiAider,
  claudeApiRooCode,
  // ─────────────────────────── COMPARE ───────────────────────────
  apitokenVsAnthropicDirect,
  apitokenVsOpenrouter,
  claudeOpusVsSonnet,
  // ─────────────────────────── EXPLAIN ───────────────────────────
  claudeApiPricingExplained,
  saveTokensOnClaudeApi,
  howBillingWorks,
  claudeApiActivationTime,
  claudeApiSupportedCountries,
  claudeApiRefundPolicy,
  // ─────────────────────────── COMPARE (expansion) ───────────────────────────
  apitokenVsProxyapi,
  apitokenVsPortkey,
  apitokenVsLitellm,
  bestClaudeModelForCoding,
  claudeMaxPlanVsApi,
  claude35VsClaude4,
  whyChooseApitoken,
  // ─────────────────────────── EXPLAIN (expansion) ───────────────────────────
  claudeApiGateway,
  claudeApiRateLimits,
  claudeApiStreaming,
  claudeApiPromptCaching,
  claudeApiBestPractices,
  // ─────────────────────────── INTEGRATE (expansion) ───────────────────────────
  claudeCodeApiKey,
  openaiApiQuickstart,
  codexCliSetup,
  vscodeAiAgentsOnePrompt,
  claudeApiKeySecurity,
  claudeApiForAiAgents,
];
