export const DOCS_URL = process.env.NEXT_PUBLIC_DOCS_URL?.trim() || "/docs";

export function isExternalDocsUrl(): boolean {
  return /^https?:\/\//i.test(DOCS_URL);
}
