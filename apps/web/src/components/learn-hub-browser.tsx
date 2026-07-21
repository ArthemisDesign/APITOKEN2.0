"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import type { LearnCluster, Locale } from "@/lib/learn";

type HubArticle = {
  slug: string;
  cluster: LearnCluster;
  title: string;
  description: string;
  href: string;
};

type HubLabels = Record<LearnCluster, { label: string; blurb: string }>;

const copy: Record<Locale, {
  search: string;
  all: string;
  results: string;
  empty: string;
  clear: string;
  topics: string;
}> = {
  en: { search: "Search guides", all: "All topics", results: "{count} guides", empty: "No guides match your search.", clear: "Clear filters", topics: "Guide topics" },
  ru: { search: "Поиск по руководствам", all: "Все темы", results: "Руководств: {count}", empty: "По вашему запросу руководства не найдены.", clear: "Сбросить фильтры", topics: "Темы руководств" },
  zh: { search: "搜索指南", all: "所有主题", results: "{count} 篇指南", empty: "没有符合条件的指南。", clear: "清除筛选", topics: "指南主题" },
  ko: { search: "가이드 검색", all: "모든 주제", results: "가이드 {count}개", empty: "검색과 일치하는 가이드가 없습니다.", clear: "필터 지우기", topics: "가이드 주제" },
};

export function filterHubArticles(articles: HubArticle[], query: string, cluster: LearnCluster | "all"): HubArticle[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  return articles.filter((article) => {
    if (cluster !== "all" && article.cluster !== cluster) return false;
    if (!normalizedQuery) return true;
    return `${article.title} ${article.description}`.toLocaleLowerCase().includes(normalizedQuery);
  });
}

export function LearnHubBrowser({ locale, clusterOrder, labels, articles }: {
  locale: Locale;
  clusterOrder: LearnCluster[];
  labels: HubLabels;
  articles: HubArticle[];
}) {
  const ui = copy[locale];
  const [query, setQuery] = useState("");
  const [cluster, setCluster] = useState<LearnCluster | "all">("all");
  const filtered = useMemo(() => filterHubArticles(articles, query, cluster), [articles, cluster, query]);
  const resultsLabel = ui.results.replace("{count}", String(filtered.length));

  return <section className="borderless learn-browser" aria-label={ui.topics}>
    <div className="wrap">
      <div className="learn-controls">
        <label className="learn-search">
          <span aria-hidden="true">⌕</span>
          <span className="sr-only">{ui.search}</span>
          <input type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={ui.search} />
        </label>
        <div className="learn-filters" role="group" aria-label={ui.topics}>
          <button type="button" aria-pressed={cluster === "all"} onClick={() => setCluster("all")}>{ui.all}</button>
          {clusterOrder.map((entry) => <button type="button" aria-pressed={cluster === entry} onClick={() => setCluster(entry)} key={entry}>{labels[entry].label}</button>)}
        </div>
        <span className="learn-result-count" role="status" aria-live="polite">{resultsLabel}</span>
      </div>

      {filtered.length === 0 ? <div className="learn-empty">
        <p>{ui.empty}</p>
        <button className="btn btn-ghost btn-sm" type="button" onClick={() => { setQuery(""); setCluster("all"); }}>{ui.clear}</button>
      </div> : clusterOrder.map((entry) => {
        const items = filtered.filter((article) => article.cluster === entry);
        if (items.length === 0) return null;
        return <div className="learn-cluster" key={entry}>
          <div className="learn-cluster-head">
            <h2 className="docs-h3">{labels[entry].label}</h2>
            <p className="docs-para">{labels[entry].blurb}</p>
          </div>
          <div className="learn-grid">
            {items.map((article) => <Link className="learn-card" href={article.href} key={article.slug}>
              <strong>{article.title}</strong>
              <span>{article.description}</span>
            </Link>)}
          </div>
        </div>;
      })}
    </div>
  </section>;
}
