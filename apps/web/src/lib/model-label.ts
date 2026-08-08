const ACRONYMS: Readonly<Record<string, string>> = {
  gpt: "GPT",
  k3: "K3",
  "k3[1m]": "K3 (1M)",
};

function titleCase(part: string): string {
  const lower = part.toLowerCase();
  return ACRONYMS[lower] ?? `${part[0]!.toUpperCase()}${part.slice(1)}`;
}

function joinNumericParts(parts: string[]): string[] {
  const result: string[] = [];

  for (const part of parts) {
    if (/^\d+$/.test(part) && /^\d+(?:\.\d+)*$/.test(result.at(-1) ?? "")) {
      result[result.length - 1] = `${result.at(-1)}.${part}`;
    } else {
      result.push(part);
    }
  }

  return result;
}

export function modelLabel(id: string): string {
  const base = id.replace(/-\d{8}$/, "");
  const parts = base.split("-").filter(Boolean);

  // Claude model IDs historically put the family after the version
  // (claude-3-5-sonnet), while the dashboard presents family first.
  if (parts[0]?.toLowerCase() === "claude") {
    const words: string[] = [];
    const numbers: string[] = [];
    for (const part of parts.slice(1)) {
      if (/^\d+$/.test(part)) numbers.push(part);
      else words.push(titleCase(part));
    }
    return `Claude ${words.join(" ")}${numbers.length ? ` ${numbers.join(".")}` : ""}`.trim();
  }

  return joinNumericParts(parts).map(titleCase).join(" ");
}

/** Billing provider behind a public model ID — display grouping only, never pricing input. */
export function modelProvider(id: string): "anthropic" | "openai" | "gemini" | "kimi" | "other" {
  // Unified-router ids arrive namespaced (`kimi/k3`); usage events carry the bare alias. Accept
  // both so the same model is not grouped two different ways depending on where the id came from.
  const bare = id.includes("/") ? id.slice(id.indexOf("/") + 1) : id;
  const family = bare.split("-", 1)[0]?.toLowerCase();
  if (family === "claude") return "anthropic";
  if (family === "gpt") return "openai";
  if (family === "gemini") return "gemini";
  // KIMI publishes two unrelated-looking alias shapes: `kimi-for-coding…` and the bare `k3`
  // family (`k3`, `k3[1m]`, `k3-256k`). Matching only the first would send half the catalog to
  // the neutral bucket.
  if (family === "kimi" || family === "k3" || family === "k3[1m]") return "kimi";
  return "other";
}
