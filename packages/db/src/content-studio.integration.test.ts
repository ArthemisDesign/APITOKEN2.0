import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  BlogFirstPublicationError,
  createContentProject,
  createDatabase,
  getContentProject,
  getPublishedBlogPost,
  InvalidContentWorkflowError,
  listPublishedBlogPosts,
  publishBlogPost,
  recordExternalPublication,
  reviseContentDraft,
  updateContentProject,
  updateContentDraft,
  upsertContentDraft,
  type Database,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("content studio persistence", () => {
  let database: Database;
  let projectId: string;
  let blogDraftId: string;
  let redditDraftId: string;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE external_publications, blog_posts, content_revisions, content_drafts,
        content_sources, content_projects, platform_profiles RESTART IDENTITY CASCADE
    `);
    projectId = await createContentProject(database, {
      sourceUrl: "https://x.com/example/status/123",
      sourcePlatform: "x",
      sourceTitle: "New model announcement",
      sourceAuthor: "Example AI",
      sourceContent: "A new model is available today.",
      sourcePublishedAt: new Date("2026-07-20T10:00:00Z"),
      sourceSnapshot: { postId: "123" },
      primaryLocale: "en",
    });
    await updateContentProject(database, projectId, { briefMarkdown: "# Verified brief\n\nConfirmed by official docs." });
    blogDraftId = await upsertContentDraft(database, {
      projectId, profileKey: "blog", locale: "en", briefVersion: 1,
      document: { title: "What the new model changes", excerpt: "A verified developer summary.", bodyMarkdown: "# Result\n\nOur measured result." },
    });
    redditDraftId = await upsertContentDraft(database, {
      projectId, profileKey: "reddit", locale: "en", briefVersion: 1,
      document: { title: "I tested the new model", excerpt: "Results for developers.", bodyMarkdown: "Here is what changed.\n\n{{CANONICAL_URL}}" },
    });
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE external_publications, blog_posts, content_revisions, content_drafts,
        content_sources, content_projects, platform_profiles RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  it("stores the shared brief and independent platform drafts", async () => {
    const project = await getContentProject(database, projectId);
    expect(project).toMatchObject({ status: "drafting", brief_version: 1 });
    expect(project!.sources).toHaveLength(1);
    expect(project!.drafts).toHaveLength(2);
  });

  it("records scoped revisions without changing another platform draft", async () => {
    await reviseContentDraft(database, {
      draftId: redditDraftId,
      instruction: "Put the benchmark first",
      scope: "draft",
      document: { title: "Benchmark first", excerpt: "Measured results.", bodyMarkdown: "The benchmark comes first." },
    });
    const rows = await database.pool.query("SELECT profile_key, title, revision FROM content_drafts ORDER BY profile_key");
    expect(rows.rows).toEqual([
      { profile_key: "blog", title: "What the new model changes", revision: 1 },
      { profile_key: "reddit", title: "Benchmark first", revision: 2 },
    ]);
  });

  it("prohibits external publication in both the service and database until the blog is live", async () => {
    await expect(recordExternalPublication(database, {
      draftId: redditDraftId,
      url: "https://reddit.com/r/test/comments/abc",
    })).rejects.toBeInstanceOf(BlogFirstPublicationError);

    await expect(database.pool.query(`
      INSERT INTO external_publications (id, project_id, draft_id, platform_key, url)
      VALUES ($1,$2,$3,'reddit','https://reddit.com/r/test/comments/direct')
    `, [randomUUID(), projectId, redditDraftId])).rejects.toMatchObject({ code: "23514" });
  });

  it("publishes the canonical article first and then unlocks external distribution", async () => {
    const post = await publishBlogPost(database, {
      projectId,
      draftId: blogDraftId,
      slug: "new-model-developer-test",
      authorName: "apiToken.sale Editorial",
      seoTitle: "New model developer test",
      seoDescription: "Verified model changes, measured results and migration advice.",
      relatedPaths: ["/docs/learn/claude-api-best-practices"],
      canonicalUrl: "https://apitoken.sale/blog/new-model-developer-test",
    });
    expect(post).toMatchObject({ status: "published", slug: "new-model-developer-test" });
    const hydrated = await database.pool.query("SELECT body_markdown, revision FROM content_drafts WHERE id = $1", [redditDraftId]);
    expect(hydrated.rows[0]).toEqual({
      body_markdown: "Here is what changed.\n\nhttps://apitoken.sale/blog/new-model-developer-test",
      revision: 2,
    });

    await expect(recordExternalPublication(database, {
      draftId: redditDraftId,
      url: "https://reddit.com/r/test/comments/abc",
    })).resolves.toMatchObject({ platform_key: "reddit" });

    await expect(listPublishedBlogPosts(database)).resolves.toHaveLength(1);
    await expect(getPublishedBlogPost(database, "new-model-developer-test", "en"))
      .resolves.toMatchObject({ title: "What the new model changes" });
  });

  it("refuses to record an external post after its canonical backlink is removed", async () => {
    await publishBlogPost(database, {
      projectId,
      draftId: blogDraftId,
      slug: "new-model-developer-test",
      authorName: "apiToken.sale Editorial",
      seoTitle: "New model developer test",
      seoDescription: "Verified model changes, measured results and migration advice.",
      relatedPaths: [],
      canonicalUrl: "https://apitoken.sale/blog/new-model-developer-test",
    });
    await updateContentDraft(database, redditDraftId, { bodyMarkdown: "Useful text, but no canonical link." });
    await expect(recordExternalPublication(database, {
      draftId: redditDraftId,
      url: "https://reddit.com/r/test/comments/no-backlink",
    })).rejects.toBeInstanceOf(InvalidContentWorkflowError);
  });
});
