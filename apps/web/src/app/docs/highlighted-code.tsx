"use client";

import { useMemo } from "react";
import { highlightCode, type HighlightToken } from "./integration-highlight";

// Renders highlighted code as numbered lines. The flat token stream is grouped
// on "\n" tokens; the visible number comes from CSS counters, so copied text
// stays exactly the source.
export function HighlightedCode({ code }: { code: string }) {
  const lines = useMemo(() => {
    const grouped: HighlightToken[][] = [[]];
    for (const part of highlightCode(code)) {
      if (part.text === "\n") grouped.push([]);
      else grouped[grouped.length - 1].push(part);
    }
    return grouped;
  }, [code]);
  return <>{lines.map((line, lineIndex) => <span className="tk-line" key={lineIndex}>{line.map((part, partIndex) => part.cls
    ? <span key={partIndex} className={`tk-${part.cls}`}>{part.text}</span>
    : <span key={partIndex}>{part.text}</span>)}</span>)}</>;
}
