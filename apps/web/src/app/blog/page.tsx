import type { Metadata } from "next";
import Link from "next/link";
import { JsonLd } from "@/components/json-ld";
import { blogPath, listBlogPosts } from "@/lib/blog";
import { absoluteUrl, createPageMetadata } from "@/lib/seo";

export const revalidate = 60;
export const metadata: Metadata = createPageMetadata({
  path: "/blog",
  title: "AI API News, Tests & Practical Analysis",
  description: "Verified AI API news, practical model analysis, benchmarks and implementation advice from the apiToken.sale editorial team.",
});

export default async function BlogPage() {
  const posts = await listBlogPosts();
  return <main className="blog-hub">
    <JsonLd data={{ "@context": "https://schema.org", "@type": "CollectionPage", name: "apiToken.sale Blog", url: absoluteUrl("/blog"), mainEntity: posts.map((post) => ({ "@type": "BlogPosting", headline: post.title, url: absoluteUrl(blogPath(post)), datePublished: post.published_at })) }} />
    <div className="page-hero"><div className="wrap"><span className="eyebrow">Verified, useful, original</span><h1>AI API field notes</h1><p>What changed, what the original source actually says, and what developers should do next.</p></div></div>
    <section className="borderless"><div className="wrap blog-grid">
      {posts.length === 0 && <div className="blog-empty"><h2>No field notes published yet</h2><p>Use the maintained guide library and model catalog for product documentation and practical setup help.</p><div className="hero-cta page-actions" style={{ justifyContent: "center" }}><Link className="btn btn-primary" href="/docs/learn">Browse guides</Link><Link className="btn btn-ghost" href="/models">Compare models</Link><Link className="btn btn-ghost" href="/changelog">Recent changes</Link></div></div>}
      {posts.map((post) => <article className="blog-card" key={post.id}><div className="blog-card-meta"><time dateTime={post.published_at}>{formatDate(post.published_at, post.locale)}</time><span>{post.locale.toUpperCase()}</span></div><h2><Link href={blogPath(post)}>{post.title}</Link></h2><p>{post.excerpt}</p><Link className="blog-read" href={blogPath(post)}>Read the full analysis →</Link></article>)}
    </div></section>
  </main>;
}

function formatDate(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale === "ru" ? "ru-RU" : "en-US", { dateStyle: "medium", timeZone: "UTC" }).format(new Date(value));
}
