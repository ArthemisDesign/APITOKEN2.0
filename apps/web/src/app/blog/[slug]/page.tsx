import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { BlogMarkdown } from "@/components/blog-markdown";
import { JsonLd } from "@/components/json-ld";
import { blogPath, getBlogPost } from "@/lib/blog";
import { absoluteUrl, createNoIndexMetadata, DEFAULT_OG_IMAGE, SITE_NAME } from "@/lib/seo";

export const revalidate = 60;

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const post = await getBlogPost(slug);
  if (!post) return createNoIndexMetadata("Article not found", "The requested article does not exist.");
  const canonical = absoluteUrl(blogPath(post));
  return {
    title: post.seo_title,
    description: post.seo_description,
    authors: [{ name: post.author_name }],
    alternates: { canonical },
    openGraph: { type: "article", url: canonical, title: post.seo_title, description: post.seo_description, siteName: SITE_NAME, publishedTime: post.published_at, modifiedTime: post.updated_at, authors: [post.author_name], images: [DEFAULT_OG_IMAGE] },
    twitter: { card: "summary_large_image", title: post.seo_title, description: post.seo_description, images: [DEFAULT_OG_IMAGE] },
  };
}

export default async function BlogPostPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const post = await getBlogPost(slug);
  if (!post) notFound();
  const canonical = absoluteUrl(blogPath(post));
  const jsonLd = { "@context": "https://schema.org", "@type": "Article", mainEntityOfPage: canonical, headline: post.title, description: post.seo_description, datePublished: post.published_at, dateModified: post.updated_at, author: { "@type": "Organization", name: post.author_name, url: absoluteUrl("/about") }, publisher: { "@type": "Organization", name: SITE_NAME, url: absoluteUrl("/") }, citation: post.source_urls };
  return <main className="blog-article"><JsonLd data={jsonLd} /><header className="blog-article-head"><div className="wrap blog-reading"><Link className="auth-back" href="/blog">← All field notes</Link><span className="eyebrow">AI API field notes</span><h1>{post.title}</h1><p className="blog-deck">{post.excerpt}</p><div className="blog-byline"><span>By {post.author_name}</span><time dateTime={post.published_at}>{new Date(post.published_at).toLocaleDateString("en-US", { dateStyle: "long", timeZone: "UTC" })}</time></div></div></header>
    <article className="wrap blog-reading blog-content"><BlogMarkdown markdown={post.body_markdown} />
      <section className="blog-sources"><h2>Primary sources</h2><p>These are the original materials used for the factual brief. apiToken.sale is responsible for the analysis above.</p><ul>{post.source_urls.map((url) => <li key={url}><a href={url} target="_blank" rel="noreferrer">{sourceLabel(url)}</a></li>)}</ul></section>
      {post.related_paths.length > 0 && <section className="blog-related"><h2>Continue reading</h2><div>{post.related_paths.map((path) => <Link href={path} key={path}>{path.replace(/^\//, "").replaceAll("-", " ")} →</Link>)}</div></section>}
    </article>
  </main>;
}

function sourceLabel(value: string): string { try { const url = new URL(value); return `${url.hostname.replace(/^www\./, "")} — ${value}`; } catch { return value; } }
