export const DOCS_URL = process.env.NEXT_PUBLIC_DOCS_URL?.trim() || "/docs";

export const GITHUB_URL = "https://github.com/apitokensale-admin";

export function isExternalDocsUrl(): boolean {
  return /^https?:\/\//i.test(DOCS_URL);
}
