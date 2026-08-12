type InvalidationListener = (prefixes: readonly string[]) => void;

const listeners = new Set<InvalidationListener>();

export function resourceMatches(url: string, prefix: string): boolean {
  const candidate = url.split("?", 1)[0]?.split("#", 1)[0] ?? url;
  const normalized = (prefix.split("?", 1)[0]?.split("#", 1)[0] ?? prefix).replace(/\/$/, "");
  return candidate === normalized || candidate.startsWith(normalized + "/");
}

export function publishInvalidation(prefixes: readonly string[]): void {
  const valid = prefixes.filter((prefix) => prefix.startsWith("/") && prefix.length <= 256);
  if (!valid.length) return;
  for (const listener of listeners) listener(valid);
}

export function subscribeInvalidations(listener: InvalidationListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
