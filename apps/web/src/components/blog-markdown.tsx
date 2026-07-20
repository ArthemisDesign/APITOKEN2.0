import type { ReactNode } from "react";

export function BlogMarkdown({ markdown }: { markdown: string }) {
  const blocks = markdown.replace(/\r\n/g, "\n").split(/\n{2,}/);
  return <div className="blog-prose">{blocks.map((block, index) => renderBlock(block.trim(), index))}</div>;
}

function renderBlock(block: string, key: number): ReactNode {
  if (!block) return null;
  const heading = block.match(/^(#{1,4})\s+(.+)$/s);
  if (heading) {
    const children = inline(heading[2]!);
    if (heading[1]!.length === 1) return <h2 key={key}>{children}</h2>;
    if (heading[1]!.length === 2) return <h2 key={key}>{children}</h2>;
    return <h3 key={key}>{children}</h3>;
  }
  if (block.startsWith("```")) return <pre key={key}><code>{block.replace(/^```[^\n]*\n?/, "").replace(/```$/, "")}</code></pre>;
  const lines = block.split("\n");
  if (lines.every((line) => /^[-*]\s+/.test(line))) return <ul key={key}>{lines.map((line) => <li key={line}>{inline(line.replace(/^[-*]\s+/, ""))}</li>)}</ul>;
  if (lines.every((line) => /^\d+\.\s+/.test(line))) return <ol key={key}>{lines.map((line) => <li key={line}>{inline(line.replace(/^\d+\.\s+/, ""))}</li>)}</ol>;
  if (lines.every((line) => line.startsWith(">"))) return <blockquote key={key}>{inline(lines.map((line) => line.replace(/^>\s?/, "")).join(" "))}</blockquote>;
  return <p key={key}>{inline(lines.join(" "))}</p>;
}

function inline(value: string): ReactNode[] {
  const result: ReactNode[] = [];
  const pattern = /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s]+)/g;
  let cursor = 0;
  for (const match of value.matchAll(pattern)) {
    if (match.index! > cursor) result.push(value.slice(cursor, match.index));
    const url = match[2] ?? match[3]!;
    result.push(<a href={url} rel="noreferrer" target="_blank" key={`${match.index}-${url}`}>{match[1] ?? url}</a>);
    cursor = match.index! + match[0].length;
  }
  if (cursor < value.length) result.push(value.slice(cursor));
  return result;
}
