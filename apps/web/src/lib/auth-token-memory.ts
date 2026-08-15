type TokenPurpose = "reset-password" | "verify-email";

const tokens: Partial<Record<TokenPurpose, string>> = {};

export function rememberAuthToken(purpose: TokenPurpose, token: string): string {
  tokens[purpose] = token;
  return token;
}

export function rememberedAuthToken(purpose: TokenPurpose): string {
  return tokens[purpose] ?? "";
}

export function takeRememberedAuthToken(purpose: TokenPurpose): string {
  const token = rememberedAuthToken(purpose);
  forgetAuthToken(purpose);
  return token;
}

export function forgetAuthToken(purpose: TokenPurpose): void {
  delete tokens[purpose];
}
