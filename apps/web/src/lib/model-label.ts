const ACRONYMS: Readonly<Record<string, string>> = {
  gpt: "GPT",
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
export function modelProvider(id: string): "anthropic" | "openai" | "other" {
  const family = id.split("-", 1)[0]?.toLowerCase();
  if (family === "claude") return "anthropic";
  if (family === "gpt") return "openai";
  return "other";
}
