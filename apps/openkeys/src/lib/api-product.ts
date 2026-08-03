export const API_TYPES = ["anthropic", "openai"] as const;

export type ApiType = (typeof API_TYPES)[number];

export interface ApiProduct {
  type: ApiType;
  shortLabel: string;
  label: string;
  baseUrl: string;
  docsPath: string;
  priceLabel: string;
  balanceLabel: string;
}

export const API_PRODUCTS: Record<ApiType, ApiProduct> = {
  anthropic: {
    type: "anthropic",
    shortLabel: "Claude",
    label: "Claude / Anthropic API",
    baseUrl: "https://router.apitoken.sale",
    docsPath: "/docs/claude",
    priceLabel: "Anthropic",
    balanceLabel: "Claude API",
  },
  openai: {
    type: "openai",
    shortLabel: "GPT",
    label: "GPT / OpenAI API",
    baseUrl: "https://router.apitoken.sale/v1",
    docsPath: "/docs/openai",
    priceLabel: "GPT API",
    balanceLabel: "GPT API",
  },
};

/** Старые партии без дискриминатора навсегда остаются Claude-партиями. */
export function apiTypeOf(value: string | null | undefined): ApiType {
  return value === "openai" ? "openai" : "anthropic";
}

export function parseApiType(value: unknown): ApiType | null {
  return value === "anthropic" || value === "openai" ? value : null;
}
