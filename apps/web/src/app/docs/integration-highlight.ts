// Tiny dependency-free syntax highlighter for the integration builder's
// shell / JSON / TOML snippets. Deliberately small: four token roles
// (comment, string, keyword, variable) — enough for GitHub-grade reading
// comfort without shipping a highlighting library to the browser.
export type HighlightToken = {
  text: string;
  cls: "c" | "s" | "k" | "v" | null;
};

const SHELL_COMMANDS = new Set([
  "export", "unset", "set", "Remove-Item", "curl", "claude", "codex",
  "opencode", "pi", "hermes", "iex", "irm", "powershell", "npm", "npx",
]);

export function detectHighlightLang(code: string): "shell" | "json" | "toml" {
  const trimmed = code.trim();
  if (trimmed.startsWith("{")) return "json";
  if (/^\[?[a-zA-Z_.]+\]?\s*=|^model(_provider)?\s*=|\[model_providers\./m.test(trimmed)) return "toml";
  return "shell";
}

function token(text: string, cls: HighlightToken["cls"] = null): HighlightToken {
  return { text, cls };
}

// Order matters: strings, then $vars, then ALL_CAPS names, then words.
const SHELL_PATTERN = /("[^"\n]*"|'[^'\n]*')|(\$env:[A-Za-z_][\w]*|\$\{?[\w]+\}?|\bEnv:[A-Za-z_][\w]*|\b[A-Z][A-Z0-9_]{2,}\b)|([^\s"']+)|(\s+)/g;

function highlightShellLine(line: string, out: HighlightToken[]): void {
  if (line.trimStart().startsWith("#")) {
    out.push(token(line, "c"));
    return;
  }
  let firstWord = true;
  for (const match of line.matchAll(SHELL_PATTERN)) {
    const [, str, variable, word, space] = match;
    if (str) out.push(token(str, "s"));
    else if (variable) out.push(token(variable, "v"));
    else if (space) out.push(token(space));
    else if (word) {
      if (firstWord && SHELL_COMMANDS.has(word)) out.push(token(word, "k"));
      else if (!firstWord && (word === "irm" || word === "iex")) out.push(token(word, "k"));
      else out.push(token(word));
      firstWord = false;
    }
  }
}

const JSON_PATTERN = /("(?:[^"\\]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?|\btrue\b|\bfalse\b|\bnull\b)|(\s+)|(.)/g;

function highlightJsonLine(line: string, out: HighlightToken[]): void {
  for (const match of line.matchAll(JSON_PATTERN)) {
    const [, str, colon, literal, space, other] = match;
    if (str) out.push(token(str, colon ? "v" : "s"), ...(colon ? [token(colon)] : []));
    else if (literal) out.push(token(literal, "k"));
    else if (space) out.push(token(space));
    else out.push(token(other));
  }
}

function highlightTomlLine(line: string, out: HighlightToken[]): void {
  const trimmed = line.trimStart();
  if (trimmed.startsWith("#")) {
    out.push(token(line, "c"));
    return;
  }
  if (trimmed.startsWith("[")) {
    out.push(token(line, "k"));
    return;
  }
  const eq = line.indexOf("=");
  if (eq === -1) {
    out.push(token(line));
    return;
  }
  out.push(token(line.slice(0, eq), "v"), token("="));
  const value = line.slice(eq + 1);
  const valueMatch = value.match(/^(\s*)("[^"\n]*")?(.*)$/);
  if (valueMatch) {
    const [, lead, str, rest] = valueMatch;
    if (lead) out.push(token(lead));
    if (str) out.push(token(str, "s"));
    if (rest) out.push(token(rest));
  }
}

export function highlightCode(code: string): HighlightToken[] {
  const lang = detectHighlightLang(code);
  const out: HighlightToken[] = [];
  const lines = code.split("\n");
  lines.forEach((line, index) => {
    if (index > 0) out.push(token("\n"));
    if (lang === "json") highlightJsonLine(line, out);
    else if (lang === "toml") highlightTomlLine(line, out);
    else highlightShellLine(line, out);
  });
  return out;
}
