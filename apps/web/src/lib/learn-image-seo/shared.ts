import type {
  LearnArticle,
  LearnBlock,
  LearnCluster,
  LearnFaq,
  LearnSection,
  Locale,
  LocalizedContent,
} from "../learn";

export type I18n<T> = Record<Locale, T>;

export type ImageSeoSpec = {
  slug: string;
  cluster: LearnCluster;
  related: string[];
  title: I18n<string>;
  h1: I18n<string>;
  description: I18n<string>;
  keywords: I18n<string[]>;
  dek: I18n<string>;
  sections: I18n<LearnSection>[];
  faq: I18n<LearnFaq>[];
};

export const ROUTER = "https://router.apitoken.sale";
export const OPENAI = `${ROUTER}/v1`;

export function tr<T>(en: T, ru: T, zh: T, ko: T): I18n<T> {
  return { en, ru, zh, ko };
}

export function paragraph(en: string, ru: string, zh: string, ko: string): I18n<LearnBlock> {
  return tr(
    { type: "p", text: en },
    { type: "p", text: ru },
    { type: "p", text: zh },
    { type: "p", text: ko },
  );
}

export function note(en: string, ru: string, zh: string, ko: string): I18n<LearnBlock> {
  return tr(
    { type: "note", text: en },
    { type: "note", text: ru },
    { type: "note", text: zh },
    { type: "note", text: ko },
  );
}

export function list(en: string[], ru: string[], zh: string[], ko: string[]): I18n<LearnBlock> {
  return tr(
    { type: "list", items: en },
    { type: "list", items: ru },
    { type: "list", items: zh },
    { type: "list", items: ko },
  );
}

export function steps(en: string[], ru: string[], zh: string[], ko: string[]): I18n<LearnBlock> {
  return tr(
    { type: "steps", items: en },
    { type: "steps", items: ru },
    { type: "steps", items: zh },
    { type: "steps", items: ko },
  );
}

export function table(
  en: { headers: string[]; rows: string[][] },
  ru: { headers: string[]; rows: string[][] },
  zh: { headers: string[]; rows: string[][] },
  ko: { headers: string[]; rows: string[][] },
): I18n<LearnBlock> {
  return tr(
    { type: "table", ...en },
    { type: "table", ...ru },
    { type: "table", ...zh },
    { type: "table", ...ko },
  );
}

export function sharedCode(code: string): I18n<LearnBlock> {
  const block: LearnBlock = { type: "code", code };
  return tr(block, block, block, block);
}

export function section(
  heading: I18n<string>,
  blocks: I18n<LearnBlock>[],
): I18n<LearnSection> {
  return tr(
    { h2: heading.en, blocks: blocks.map((block) => block.en) },
    { h2: heading.ru, blocks: blocks.map((block) => block.ru) },
    { h2: heading.zh, blocks: blocks.map((block) => block.zh) },
    { h2: heading.ko, blocks: blocks.map((block) => block.ko) },
  );
}

export function faq(
  question: I18n<string>,
  answer: I18n<string>,
): I18n<LearnFaq> {
  return tr(
    { q: question.en, a: answer.en },
    { q: question.ru, a: answer.ru },
    { q: question.zh, a: answer.zh },
    { q: question.ko, a: answer.ko },
  );
}
