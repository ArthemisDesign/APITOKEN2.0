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
  "pip",
]);

const CODE_KEYWORDS = new Set([
  "import", "from", "export", "const", "let", "var", "await", "async", "new",
  "for", "in", "of", "if", "else", "elif", "return", "def", "class", "print",
  "function", "this", "not", "and", "or", "with", "as", "is", "while",
  "break", "continue", "pass", "lambda", "try", "except", "finally",
]);

export function detectHighlightLang(code: string): "shell" | "json" | "toml" | "code" {
  const trimmed = code.trim();
  if (trimmed.startsWith("{")) return "json";
  if (/^\[?[a-zA-Z_.]+\]?\s*=|^model(_provider)?\s*=|\[model_providers\./m.test(trimmed)) return "toml";
  if (/^(?:import|from|const|let|await|async|function|def|class)\s/m.test(trimmed)) return "code";
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

const CODE_PATTERN = /(#[^\n]*|\/\/[^\n]*)|("[^"\n]*"|'[^'\n]*'|`[^`\n]*`)|([A-Za-z_][\w]*)|(\s+)|(.)/g;

function highlightCodeLine(line: string, out: HighlightToken[]): void {
  for (const match of line.matchAll(CODE_PATTERN)) {
    const [, comment, str, word, space, other] = match;
    if (comment) out.push(token(comment, "c"));
    else if (str) out.push(token(str, "s"));
    else if (word) out.push(token(word, CODE_KEYWORDS.has(word) ? "k" : null));
    else if (space) out.push(token(space));
    else out.push(token(other));
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
    else if (lang === "code") highlightCodeLine(line, out);
    else highlightShellLine(line, out);
  });
  return out;
}
