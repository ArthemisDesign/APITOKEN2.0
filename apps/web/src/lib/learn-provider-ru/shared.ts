import type { LearnBlock } from "../learn";
import { learnProviderEn } from "../learn-provider-en";

// Reuses the English provider article blocks (code samples stay untranslated).
export function sourceBlock(slug: string, sectionIndex: number, blockIndex: number): LearnBlock {
  const article = learnProviderEn.find((entry) => entry.slug === slug);
  if (!article) throw new Error("Unknown provider guide: " + slug);
  const block = article.sections[sectionIndex]?.blocks[blockIndex];
  if (!block) throw new Error("Missing provider guide block: " + slug + "/" + sectionIndex + "/" + blockIndex);
  return block;
}
