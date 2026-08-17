// Provider-specific SEO expansion (GPT, Gemini, Kimi), one module per article.
// Composed here in the original order from learn-provider-en.ts.

import type { LearnArticle } from "../learn";
import { article as howToBuyGptApiKey } from "./how-to-buy-gpt-api-key";
import { article as gptApiPricing } from "./gpt-api-pricing";
import { article as gpt56SolVsTerraVsLuna } from "./gpt-5-6-sol-vs-terra-vs-luna";
import { article as gptImage2ApiGuide } from "./gpt-image-2-api-guide";
import { article as howToBuyGeminiApiKey } from "./how-to-buy-gemini-api-key";
import { article as geminiApiQuickstart } from "./gemini-api-quickstart";
import { article as geminiApiPricing } from "./gemini-api-pricing";
import { article as geminiProVsFlashVsFlashLite } from "./gemini-pro-vs-flash-vs-flash-lite";
import { article as nanoBanana2ApiGuide } from "./nano-banana-2-api-guide";
import { article as howToBuyKimiApiKey } from "./how-to-buy-kimi-api-key";
import { article as kimiApiQuickstart } from "./kimi-api-quickstart";
import { article as kimiApiPricing } from "./kimi-api-pricing";
import { article as kimiK3VsKimiForCoding } from "./kimi-k3-vs-kimi-for-coding";
import { article as kimiApiForOpencode } from "./kimi-api-for-opencode";
import { article as kimiApiForClaudeCode } from "./kimi-api-for-claude-code";
import { article as kimiApiForKimiCode } from "./kimi-api-for-kimi-code";

export const learnProviderEn: LearnArticle[] = [
  howToBuyGptApiKey,
  gptApiPricing,
  gpt56SolVsTerraVsLuna,
  gptImage2ApiGuide,
  howToBuyGeminiApiKey,
  geminiApiQuickstart,
  geminiApiPricing,
  geminiProVsFlashVsFlashLite,
  nanoBanana2ApiGuide,
  howToBuyKimiApiKey,
  kimiApiQuickstart,
  kimiApiPricing,
  kimiK3VsKimiForCoding,
  kimiApiForOpencode,
  kimiApiForClaudeCode,
  kimiApiForKimiCode,
];
