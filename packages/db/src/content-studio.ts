import { randomUUID } from "node:crypto";
import type { ContentLocale, ContentRevisionScope, PlatformProfileRules } from "@claude-api/contracts";
import type { PoolClient, QueryResultRow } from "pg";
import type { Database } from "./client.js";

export const BUILT_IN_PLATFORM_PROFILES: Array<{
  key: string;
  name: string;
  rules: PlatformProfileRules;
}> = [
  { key: "blog", name: "apiToken.sale blog", rules: { tone: "Evidence-led, practical and calm", audience: "AI API developers and buyers", length: "1,000-1,800 words", linkPolicy: "Cite every primary source and link to relevant apiToken.sale guides", requiredDisclosure: "", forbidden: ["invented facts", "generic AI introductions", "fake urgency"] } },
  { key: "reddit", name: "Reddit", rules: { tone: "Direct, useful and conversational", audience: "A relevant subreddit community", length: "500-1,000 words", linkPolicy: "One supporting deep link only when community rules allow it", requiredDisclosure: "Disclosure: I run apiToken.sale.", forbidden: ["sales pitch", "link farming", "repeated brand mentions"] } },
  { key: "vc-ru", name: "vc.ru", rules: { language: "ru", tone: "Russian business case study with concrete numbers", audience: "Russian founders, product people and developers", length: "800-1,500 words", linkPolicy: "Link to the full method or benchmark, not the homepage", requiredDisclosure: "Материал подготовлен командой apiToken.sale.", forbidden: ["агрессивная реклама", "бездоказательные заявления", "копипаст"] } },
  { key: "dzen", name: "Dzen", rules: { language: "ru", tone: "Clear and accessible without losing technical accuracy", audience: "Russian readers interested in practical AI", length: "700-1,300 words", linkPolicy: "Use one contextual link to the full analysis", requiredDisclosure: "", forbidden: ["clickbait", "copied source text", "unsupported claims"] } },
  { key: "habr", name: "Habr", rules: { language: "ru", tone: "Technical, transparent and method-first", audience: "Experienced Russian-speaking engineers", length: "1,200-2,500 words", linkPolicy: "Only useful non-referral links; raw data and code are preferred", requiredDisclosure: "Автор связан с apiToken.sale.", forbidden: ["press release", "empty promotion", "hidden affiliation"] } },
  { key: "medium", name: "Medium", rules: { tone: "Polished technical publication", audience: "English-speaking AI developers", length: "900-1,600 words", linkPolicy: "Use apiToken.sale as canonical when importing the complete article", requiredDisclosure: "", forbidden: ["keyword stuffing", "uncited claims"] } },
  { key: "x", name: "X", rules: { tone: "Compact, factual and interesting", audience: "AI builders following current releases", length: "One post or a short thread", linkPolicy: "Link to the canonical blog article", requiredDisclosure: "", forbidden: ["engagement bait", "misleading certainty"] } },
  { key: "telegram", name: "Telegram", rules: { tone: "Concise and useful", audience: "Subscribed AI developers", length: "200-500 words", linkPolicy: "Link to the canonical blog article", requiredDisclosure: "", forbidden: ["overlong preamble", "fake urgency"] } },
  { key: "linkedin", name: "LinkedIn", rules: { tone: "Professional, personal and evidence-led", audience: "AI product and engineering professionals", length: "300-700 words", linkPolicy: "Link to the canonical article after the useful summary", requiredDisclosure: "", forbidden: ["corporate filler", "engagement bait"] } },
];

export class ContentProjectNotFoundError extends Error {}
export class ContentDraftNotFoundError extends Error {}
export class BlogFirstPublicationError extends Error {}
export class InvalidContentWorkflowError extends Error {}

export interface ExtractedContentSource {
  sourceUrl: string;
  sourcePlatform: string;
  sourceTitle: string;
  sourceAuthor: string | null;
  sourceContent: string;
  sourcePublishedAt: Date | null;
  sourceSnapshot: Record<string, unknown>;
  primaryLocale: ContentLocale;
}

export interface DraftDocument {
  title: string;
  excerpt: string;
  bodyMarkdown: string;
}

export async function createContentProject(database: Database, input: ExtractedContentSource): Promise<string> {
  const id = randomUUID();
  await database.pool.query(`
    INSERT INTO content_projects (
      id, source_url, source_platform, source_title, source_author, source_content,
      source_published_at, source_snapshot, primary_locale
    ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::jsonb,$9)
  `, [id, input.sourceUrl, input.sourcePlatform, input.sourceTitle, input.sourceAuthor,
    input.sourceContent, input.sourcePublishedAt, JSON.stringify(input.sourceSnapshot), input.primaryLocale]);
  await addContentSource(database, id, {
    url: input.sourceUrl,
    title: input.sourceTitle,
    sourceType: "primary",
    publisher: input.sourceAuthor,
    notes: "Imported source",
  });
  return id;
}

export async function listContentProjects(database: Database, limit = 100): Promise<Record<string, unknown>[]> {
  const result = await database.pool.query(`
    SELECT project.*,
      post.slug AS blog_slug,
      post.published_at AS blog_published_at,
      (SELECT count(*)::int FROM content_drafts draft WHERE draft.project_id = project.id) AS draft_count,
      (SELECT count(*)::int FROM external_publications publication WHERE publication.project_id = project.id) AS publication_count
    FROM content_projects project
    LEFT JOIN blog_posts post ON post.project_id = project.id AND post.status = 'published'
    ORDER BY project.updated_at DESC
    LIMIT $1
  `, [limit]);
  return result.rows;
}

export async function getContentProject(database: Database, projectId: string): Promise<Record<string, unknown> | null> {
  const [project, sources, drafts, post, publications] = await Promise.all([
    database.pool.query("SELECT * FROM content_projects WHERE id = $1", [projectId]),
    database.pool.query("SELECT * FROM content_sources WHERE project_id = $1 ORDER BY created_at", [projectId]),
    database.pool.query("SELECT * FROM content_drafts WHERE project_id = $1 ORDER BY profile_key, locale", [projectId]),
    database.pool.query("SELECT * FROM blog_posts WHERE project_id = $1", [projectId]),
    database.pool.query("SELECT * FROM external_publications WHERE project_id = $1 ORDER BY published_at", [projectId]),
  ]);
  if (!project.rows[0]) return null;
  return { ...project.rows[0], sources: sources.rows, drafts: drafts.rows, blog_post: post.rows[0] ?? null, publications: publications.rows };
}

export async function updateContentProject(database: Database, projectId: string, input: {
  sourceTitle?: string;
  sourceAuthor?: string | null;
  sourceContent?: string;
  briefMarkdown?: string;
}): Promise<void> {
  const fields: string[] = [];
  const values: unknown[] = [];
  const add = (column: string, value: unknown): void => { values.push(value); fields.push(`${column} = $${values.length}`); };
  if (input.sourceTitle !== undefined) add("source_title", input.sourceTitle);
  if (input.sourceAuthor !== undefined) add("source_author", input.sourceAuthor);
  if (input.sourceContent !== undefined) add("source_content", input.sourceContent);
  if (input.briefMarkdown !== undefined) {
    add("brief_markdown", input.briefMarkdown);
    add("status", input.briefMarkdown.trim() ? "brief_ready" : "imported");
    fields.push("brief_version = brief_version + 1");
  }
  if (fields.length === 0) return;
  values.push(projectId);
  const result = await database.pool.query(
    `UPDATE content_projects SET ${fields.join(", ")}, updated_at = now() WHERE id = $${values.length}`,
    values,
  );
  if (result.rowCount === 0) throw new ContentProjectNotFoundError(projectId);
}

export async function addContentSource(database: Database, projectId: string, input: {
  url: string;
  title: string;
  sourceType: "primary" | "reference" | "verification";
  publisher?: string | null;
  notes: string;
}): Promise<void> {
  await database.pool.query(`
    INSERT INTO content_sources (id, project_id, url, title, source_type, publisher, notes)
    VALUES ($1,$2,$3,$4,$5,$6,$7)
    ON CONFLICT (project_id, url) DO UPDATE SET
      title = EXCLUDED.title, source_type = EXCLUDED.source_type,
      publisher = EXCLUDED.publisher, notes = EXCLUDED.notes
  `, [randomUUID(), projectId, input.url, input.title, input.sourceType, input.publisher ?? null, input.notes]);
}

export async function ensureBuiltInPlatformProfiles(database: Database): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    for (const profile of BUILT_IN_PLATFORM_PROFILES) {
      await client.query(`
        INSERT INTO platform_profiles (id, key, name, rules, built_in)
        VALUES ($1,$2,$3,$4::jsonb,true)
        ON CONFLICT (key) DO UPDATE SET
          name = EXCLUDED.name, rules = EXCLUDED.rules, built_in = true, active = true, updated_at = now()
      `, [randomUUID(), profile.key, profile.name, JSON.stringify(profile.rules)]);
    }
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listPlatformProfiles(database: Database): Promise<Record<string, unknown>[]> {
  await ensureBuiltInPlatformProfiles(database);
  const result = await database.pool.query("SELECT * FROM platform_profiles WHERE active = true ORDER BY built_in DESC, name");
  return result.rows;
}

export async function upsertPlatformProfile(database: Database, input: {
  key: string;
  name: string;
  rules: PlatformProfileRules;
}): Promise<void> {
  await database.pool.query(`
    INSERT INTO platform_profiles (id, key, name, rules, built_in)
    VALUES ($1,$2,$3,$4::jsonb,false)
    ON CONFLICT (key) DO UPDATE SET
      name = EXCLUDED.name, rules = EXCLUDED.rules, active = true, updated_at = now()
    WHERE platform_profiles.built_in = false
  `, [randomUUID(), input.key, input.name, JSON.stringify(input.rules)]);
}

export async function upsertContentDraft(database: Database, input: {
  projectId: string;
  profileKey: string;
  locale: ContentLocale;
  briefVersion: number;
  document: DraftDocument;
}): Promise<string> {
  const id = randomUUID();
  const result = await database.pool.query<{ id: string }>(`
    INSERT INTO content_drafts (id, project_id, profile_key, locale, title, excerpt, body_markdown, brief_version)
    VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
    ON CONFLICT (project_id, profile_key, locale) DO UPDATE SET
      title = EXCLUDED.title, excerpt = EXCLUDED.excerpt, body_markdown = EXCLUDED.body_markdown,
      brief_version = EXCLUDED.brief_version, status = 'draft', revision = content_drafts.revision + 1, updated_at = now()
    RETURNING id
  `, [id, input.projectId, input.profileKey, input.locale, input.document.title,
    input.document.excerpt, input.document.bodyMarkdown, input.briefVersion]);
  await database.pool.query("UPDATE content_projects SET status = 'drafting', updated_at = now() WHERE id = $1", [input.projectId]);
  return result.rows[0]!.id;
}

export async function getContentDraft(database: Database, draftId: string): Promise<Record<string, unknown> | null> {
  const result = await database.pool.query("SELECT * FROM content_drafts WHERE id = $1", [draftId]);
  return result.rows[0] ?? null;
}

export async function reviseContentDraft(database: Database, input: {
  draftId: string;
  instruction: string;
  scope: ContentRevisionScope;
  document: DraftDocument;
}): Promise<number> {
  return withTransaction(database, async (client) => {
    const selected = await client.query("SELECT * FROM content_drafts WHERE id = $1 FOR UPDATE", [input.draftId]);
    const before = selected.rows[0];
    if (!before) throw new ContentDraftNotFoundError(input.draftId);
    const revision = Number(before.revision) + 1;
    await client.query(`
      UPDATE content_drafts SET title = $2, excerpt = $3, body_markdown = $4,
        revision = $5, status = 'draft', updated_at = now() WHERE id = $1
    `, [input.draftId, input.document.title, input.document.excerpt, input.document.bodyMarkdown, revision]);
    await client.query(`
      INSERT INTO content_revisions (id, draft_id, revision, scope, instruction, before, after)
      VALUES ($1,$2,$3,$4,$5,$6::jsonb,$7::jsonb)
    `, [randomUUID(), input.draftId, revision, input.scope, input.instruction,
      JSON.stringify(documentFromRow(before)), JSON.stringify(input.document)]);
    return revision;
  });
}

export async function updateContentDraft(database: Database, draftId: string, input: Partial<DraftDocument> & {
  status?: "draft" | "approved";
}): Promise<void> {
  const fields: string[] = [];
  const values: unknown[] = [];
  const add = (column: string, value: unknown): void => { values.push(value); fields.push(`${column} = $${values.length}`); };
  if (input.title !== undefined) add("title", input.title);
  if (input.excerpt !== undefined) add("excerpt", input.excerpt);
  if (input.bodyMarkdown !== undefined) add("body_markdown", input.bodyMarkdown);
  if (input.status !== undefined) add("status", input.status);
  values.push(draftId);
  const result = await database.pool.query(
    `UPDATE content_drafts SET ${fields.join(", ")}, updated_at = now() WHERE id = $${values.length}`,
    values,
  );
  if (result.rowCount === 0) throw new ContentDraftNotFoundError(draftId);
}

export async function publishBlogPost(database: Database, input: {
  projectId: string;
  draftId: string;
  slug: string;
  authorName: string;
  seoTitle: string;
  seoDescription: string;
  relatedPaths: string[];
  canonicalUrl: string;
}): Promise<Record<string, unknown>> {
  return withTransaction(database, async (client) => {
    const draftResult = await client.query("SELECT * FROM content_drafts WHERE id = $1 FOR UPDATE", [input.draftId]);
    const draft = draftResult.rows[0];
    if (!draft) throw new ContentDraftNotFoundError(input.draftId);
    if (draft.project_id !== input.projectId || draft.profile_key !== "blog") {
      throw new InvalidContentWorkflowError("the canonical post must use this project's blog draft");
    }
    if (!String(draft.title).trim() || !String(draft.body_markdown).trim()) {
      throw new InvalidContentWorkflowError("the blog draft must have a title and body before publishing");
    }
    const sourceResult = await client.query<{ url: string }>("SELECT url FROM content_sources WHERE project_id = $1 ORDER BY created_at", [input.projectId]);
    const id = randomUUID();
    const published = await client.query(`
      INSERT INTO blog_posts (
        id, project_id, draft_id, slug, locale, title, excerpt, body_markdown,
        author_name, seo_title, seo_description, source_urls, related_paths, status, published_at
      ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12::jsonb,$13::jsonb,'published',now())
      ON CONFLICT (project_id) DO UPDATE SET
        draft_id = EXCLUDED.draft_id, slug = EXCLUDED.slug, locale = EXCLUDED.locale,
        title = EXCLUDED.title, excerpt = EXCLUDED.excerpt, body_markdown = EXCLUDED.body_markdown,
        author_name = EXCLUDED.author_name, seo_title = EXCLUDED.seo_title,
        seo_description = EXCLUDED.seo_description, source_urls = EXCLUDED.source_urls,
        related_paths = EXCLUDED.related_paths, status = 'published',
        published_at = COALESCE(blog_posts.published_at, now()), updated_at = now()
      RETURNING *
    `, [id, input.projectId, input.draftId, input.slug, draft.locale, draft.title, draft.excerpt,
      draft.body_markdown, input.authorName, input.seoTitle, input.seoDescription,
      JSON.stringify(sourceResult.rows.map((row) => row.url)), JSON.stringify(input.relatedPaths)]);
    const externalDrafts = await client.query(
      "SELECT * FROM content_drafts WHERE project_id = $1 AND profile_key <> 'blog' FOR UPDATE",
      [input.projectId],
    );
    for (const external of externalDrafts.rows) {
      const before = documentFromRow(external);
      if (before.bodyMarkdown.includes(input.canonicalUrl)) continue;
      const bodyMarkdown = before.bodyMarkdown.includes("{{CANONICAL_URL}}")
        ? before.bodyMarkdown.replaceAll("{{CANONICAL_URL}}", input.canonicalUrl)
        : `${before.bodyMarkdown.trim()}\n\nFull analysis: ${input.canonicalUrl}`;
      const after = { ...before, bodyMarkdown };
      const revision = Number(external.revision) + 1;
      await client.query(`
        UPDATE content_drafts SET body_markdown = $2, revision = $3, status = 'draft', updated_at = now()
        WHERE id = $1
      `, [external.id, bodyMarkdown, revision]);
      await client.query(`
        INSERT INTO content_revisions (id, draft_id, revision, scope, instruction, before, after)
        VALUES ($1,$2,$3,'platform',$4,$5::jsonb,$6::jsonb)
      `, [randomUUID(), external.id, revision, "Resolved canonical blog URL after publication",
        JSON.stringify(before), JSON.stringify(after)]);
    }
    await client.query("UPDATE content_drafts SET status = 'approved', updated_at = now() WHERE id = $1", [input.draftId]);
    await client.query("UPDATE content_projects SET status = 'blog_published', updated_at = now() WHERE id = $1", [input.projectId]);
    return published.rows[0];
  });
}

export async function recordExternalPublication(database: Database, input: {
  draftId: string;
  url: string;
}): Promise<Record<string, unknown>> {
  return withTransaction(database, async (client) => {
    const draftResult = await client.query("SELECT * FROM content_drafts WHERE id = $1 FOR UPDATE", [input.draftId]);
    const draft = draftResult.rows[0];
    if (!draft) throw new ContentDraftNotFoundError(input.draftId);
    if (draft.profile_key === "blog") throw new InvalidContentWorkflowError("blog publication uses the canonical publish action");
    const blog = await client.query<{ slug: string }>("SELECT slug FROM blog_posts WHERE project_id = $1 AND status = 'published'", [draft.project_id]);
    if (!blog.rows[0]) throw new BlogFirstPublicationError("publish the apiToken.sale blog post first");
    const canonicalUrl = `https://apitoken.sale/blog/${blog.rows[0].slug}`;
    if (!String(draft.body_markdown).includes(canonicalUrl)) {
      throw new InvalidContentWorkflowError("the external draft must link to the canonical apiToken.sale article");
    }
    const publication = await client.query(`
      INSERT INTO external_publications (id, project_id, draft_id, platform_key, url)
      VALUES ($1,$2,$3,$4,$5) RETURNING *
    `, [randomUUID(), draft.project_id, input.draftId, draft.profile_key, input.url]);
    await client.query("UPDATE content_drafts SET status = 'approved', updated_at = now() WHERE id = $1", [input.draftId]);
    await client.query("UPDATE content_projects SET status = 'distributed', updated_at = now() WHERE id = $1", [draft.project_id]);
    return publication.rows[0];
  });
}

export async function listPublishedBlogPosts(database: Database, limit = 100): Promise<Record<string, unknown>[]> {
  const result = await database.pool.query(`
    SELECT id, slug, locale, title, excerpt, author_name, seo_title, seo_description,
      source_urls, related_paths, published_at, updated_at
    FROM blog_posts
    WHERE status = 'published'
    ORDER BY published_at DESC
    LIMIT $1
  `, [limit]);
  return result.rows;
}

export async function getPublishedBlogPost(database: Database, slug: string, locale: ContentLocale): Promise<Record<string, unknown> | null> {
  const result = await database.pool.query(`
    SELECT id, slug, locale, title, excerpt, body_markdown, author_name, seo_title,
      seo_description, source_urls, related_paths, published_at, updated_at
    FROM blog_posts
    WHERE slug = $1 AND locale = $2 AND status = 'published'
  `, [slug, locale]);
  return result.rows[0] ?? null;
}

function documentFromRow(row: QueryResultRow): DraftDocument {
  return { title: String(row.title), excerpt: String(row.excerpt), bodyMarkdown: String(row.body_markdown) };
}

async function withTransaction<T>(database: Database, operation: (client: PoolClient) => Promise<T>): Promise<T> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await operation(client);
    await client.query("COMMIT");
    return result;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
