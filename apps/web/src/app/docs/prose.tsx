// Renders guide prose with `backtick`-quoted fragments as inline code pills,
// matching how reference docs (OpenRouter) mark paths, headers and endpoints.
export function Prose({ text }: { text: string }) {
  const parts = text.split("`");
  return <>{parts.map((part, index) => index % 2 === 1 ? <code key={index}>{part}</code> : part)}</>;
}
