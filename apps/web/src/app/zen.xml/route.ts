// Дзен-лента: /zen.xml — специально размеченный RSS для публикации RU-версий
// learn-статей как нативных материалов Дзена (dzen.ru/help/ru/website/rss-modify.html).
//
// Подключение (разовое, руками владельца): создать канал в Дзене → Студия →
// «Сайт» → объединить сайт и канал → указать https://apitoken.sale/zen.xml.
// Дальше публикация автоматическая: новые/обновлённые статьи попадают в ленту сами.
//
// Решения, зафиксированные здесь:
// - `noindex`: копия на Дзене НЕ индексируется поиском, чтобы не каннибализировать
//   наши собственные /ru/docs/learn страницы в Яндексе. Ценность канала — лента
//   Дзена и брендовые упоминания. Поменять на "index" — одно слово в ZEN_CATEGORIES.
// - Дзен не поддерживает <table> и <pre> — таблицы рендерятся списками, код цитатой.
// - guid = slug (не permalink): правка статьи обновляет материал, а не дублирует.

import { articlesForLocale, articleUpdatedDate, learnPath, learnUi, resolveArticle, type LearnBlock } from "@/lib/learn";
import { absoluteUrl } from "@/lib/seo";

export const revalidate = 3600;

const ZEN_CATEGORIES = ["format-article", "noindex", "comment-all"];

function esc(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function blockToZenHtml(block: LearnBlock): string {
  switch (block.type) {
    case "p":
      return `<p>${esc(block.text)}</p>`;
    case "note":
      return `<blockquote><p>${esc(block.text)}</p></blockquote>`;
    case "list":
      return `<ul>${block.items.map((item) => `<li>${esc(item)}</li>`).join("")}</ul>`;
    case "steps":
      return `<ol>${block.items.map((item) => `<li>${esc(item)}</li>`).join("")}</ol>`;
    case "code":
      return `<blockquote>${block.code.split("\n").map((line) => `<p>${esc(line) || "&#160;"}</p>`).join("")}</blockquote>`;
    case "table": {
      const header = `<p><i>${esc(block.headers.join(" / "))}</i></p>`;
      const rows = block.rows
        .map((row) => `<li><b>${esc(row[0])}</b> — ${row.slice(1).map((cell) => esc(cell)).join(" / ")}</li>`)
        .join("");
      return `${header}<ul>${rows}</ul>`;
    }
    case "link":
      return `<p><a href="${esc(absoluteUrl(block.href))}">${esc(block.text)}</a></p>`;
    default:
      return "";
  }
}

function articleToItem(slug: string): string | null {
  const article = resolveArticle(slug, "ru");
  if (!article) return null;
  const { content } = article;
  const url = absoluteUrl(learnPath(slug, "ru"));
  const date = articleUpdatedDate(slug);

  const html: string[] = [`<p>${esc(content.dek)}</p>`];
  for (const section of content.sections) {
    html.push(`<h2>${esc(section.h2)}</h2>`);
    for (const block of section.blocks) html.push(blockToZenHtml(block));
  }
  if (content.faq.length > 0) {
    html.push(`<h2>${esc(learnUi.ru.faqHeading)}</h2>`);
    for (const item of content.faq) html.push(`<h3>${esc(item.q)}</h3>`, `<p>${esc(item.a)}</p>`);
  }
  html.push(`<p>Оригинал статьи — <a href="${esc(url)}">apitoken.sale</a>. Ключ и баланс — <a href="${esc(absoluteUrl("/register"))}">в личном кабинете</a>.</p>`);

  const body = html.join("\n").replaceAll("]]>", "]]&gt;");
  return [
    "    <item>",
    `      <title>${esc(content.title)}</title>`,
    `      <link>${esc(url)}</link>`,
    `      <guid isPermaLink="false">${esc(slug)}</guid>`,
    `      <pubDate>${date.toUTCString()}</pubDate>`,
    `      <description>${esc(content.description)}</description>`,
    ...ZEN_CATEGORIES.map((category) => `      <category>${category}</category>`),
    `      <enclosure url="${esc(absoluteUrl(`/ru/docs/learn/${slug}/opengraph-image`))}" type="image/png"/>`,
    `      <content:encoded><![CDATA[${body}]]></content:encoded>`,
    "    </item>",
  ].join("\n");
}

export function buildZenFeed(): string {
  const items = articlesForLocale("ru")
    .map((slug) => ({ slug, date: articleUpdatedDate(slug) }))
    .sort((a, b) => b.date.getTime() - a.date.getTime())
    .map(({ slug }) => articleToItem(slug))
    .filter((item): item is string => item !== null)
    .join("\n");

  return `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>apiToken.sale — гайды по Claude API</title>
    <link>${esc(absoluteUrl("/ru/docs/learn"))}</link>
    <description>Практические гайды по Claude API: покупка ключа, цены со скидкой до 70%, интеграции с Cursor, VS Code и Claude Code.</description>
    <language>ru</language>
    <atom:link href="${esc(absoluteUrl("/zen.xml"))}" rel="self" type="application/rss+xml"/>
${items}
  </channel>
</rss>
`;
}

export async function GET(): Promise<Response> {
  return new Response(buildZenFeed(), {
    headers: { "content-type": "application/rss+xml; charset=utf-8" },
  });
}
