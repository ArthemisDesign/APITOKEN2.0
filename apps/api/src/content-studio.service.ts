import { BadRequestException, ConflictException, Inject, Injectable, NotFoundException } from "@nestjs/common";
import { platformProfileRulesSchema, type ContentLocale, type ContentRevisionScope } from "@claude-api/contracts";
import {
  addContentSource,
  createContentProject,
  getContentDraft,
  getContentProject,
  listContentProjects,
  listPlatformProfiles,
  publishBlogPost,
  recordExternalPublication,
  reviseContentDraft,
  updateContentDraft,
  updateContentProject,
  upsertContentDraft,
  upsertPlatformProfile,
  type Database,
  type DraftDocument,
  InvalidContentWorkflowError,
} from "@claude-api/db";
import { ContentAiService } from "./content-ai.service.js";
import { ContentIndexingService } from "./content-indexing.service.js";
import { ContentSourceService } from "./content-source.service.js";
import { DATABASE } from "./infrastructure.module.js";

@Injectable()
export class ContentStudioService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly source: ContentSourceService,
    private readonly ai: ContentAiService,
    private readonly indexing: ContentIndexingService,
  ) {}

  status(): { aiEnabled: boolean; blogOrigin: string } {
    return { aiEnabled: this.ai.enabled, blogOrigin: "https://apitoken.sale/blog" };
  }

  list(): Promise<Record<string, unknown>[]> {
    return listContentProjects(this.database);
  }

  async get(projectId: string): Promise<Record<string, unknown>> {
    const project = await getContentProject(this.database, projectId);
    if (!project) throw new NotFoundException("content project not found");
    return project;
  }

  async import(input: { sourceUrl: string; locale: ContentLocale; sourceContent?: string | undefined }): Promise<Record<string, unknown>> {
    const extracted = await this.source.extract({
      sourceUrl: input.sourceUrl,
      locale: input.locale,
      ...(input.sourceContent !== undefined ? { sourceContent: input.sourceContent } : {}),
    });
    const id = await createContentProject(this.database, extracted);
    return this.get(id);
  }

  async update(projectId: string, input: {
    sourceTitle?: string | undefined; sourceAuthor?: string | null | undefined;
    sourceContent?: string | undefined; briefMarkdown?: string | undefined;
  }): Promise<Record<string, unknown>> {
    await this.get(projectId);
    await updateContentProject(this.database, projectId, {
      ...(input.sourceTitle !== undefined ? { sourceTitle: input.sourceTitle } : {}),
      ...(input.sourceAuthor !== undefined ? { sourceAuthor: input.sourceAuthor } : {}),
      ...(input.sourceContent !== undefined ? { sourceContent: input.sourceContent } : {}),
      ...(input.briefMarkdown !== undefined ? { briefMarkdown: input.briefMarkdown } : {}),
    });
    return this.get(projectId);
  }

  async addSource(projectId: string, input: {
    url: string; title: string; sourceType: "primary" | "reference" | "verification";
    publisher?: string | null | undefined; notes: string;
  }): Promise<Record<string, unknown>> {
    await this.get(projectId);
    await addContentSource(this.database, projectId, {
      url: input.url, title: input.title, sourceType: input.sourceType, notes: input.notes,
      ...(input.publisher !== undefined ? { publisher: input.publisher } : {}),
    });
    return this.get(projectId);
  }

  async generateBrief(projectId: string): Promise<Record<string, unknown>> {
    const project = await this.get(projectId);
    const sources = project.sources as Array<Record<string, unknown>>;
    const brief = await this.ai.generateBrief({
      sourceUrl: String(project.source_url), title: String(project.source_title),
      author: project.source_author ? String(project.source_author) : null,
      content: String(project.source_content), locale: project.primary_locale as ContentLocale,
      references: sources.map((entry) => ({ url: String(entry.url), title: String(entry.title), notes: String(entry.notes) })),
    });
    await updateContentProject(this.database, projectId, { briefMarkdown: brief });
    return this.get(projectId);
  }

  async profiles(): Promise<Record<string, unknown>[]> {
    return listPlatformProfiles(this.database);
  }

  async upsertProfile(input: { key: string; name: string; rules: unknown }): Promise<Record<string, unknown>[]> {
    await upsertPlatformProfile(this.database, { ...input, rules: platformProfileRulesSchema.parse(input.rules) });
    return this.profiles();
  }

  async generateDrafts(projectId: string, profileKeys: string[], locale: ContentLocale): Promise<Record<string, unknown>> {
    const project = await this.get(projectId);
    if (!String(project.brief_markdown).trim()) throw new BadRequestException("generate or write the verified brief first");
    const profiles = await this.profiles();
    const profileMap = new Map(profiles.map((profile) => [String(profile.key), profile]));
    for (const key of [...new Set(profileKeys)]) {
      const profile = profileMap.get(key);
      if (!profile) throw new NotFoundException(`platform profile not found: ${key}`);
      const document = await this.ai.generateDraft({
        brief: String(project.brief_markdown), sourceUrl: String(project.source_url),
        profileKey: key, profileName: String(profile.name),
        rules: platformProfileRulesSchema.parse(profile.rules), locale,
      });
      await upsertContentDraft(this.database, {
        projectId, profileKey: key, locale, briefVersion: Number(project.brief_version), document,
      });
    }
    return this.get(projectId);
  }

  async updateDraft(draftId: string, input: {
    title?: string | undefined; excerpt?: string | undefined; bodyMarkdown?: string | undefined;
    status?: "draft" | "approved" | undefined;
  }): Promise<Record<string, unknown>> {
    const existing = await getContentDraft(this.database, draftId);
    if (!existing) throw new NotFoundException("draft not found");
    await updateContentDraft(this.database, draftId, {
      ...(input.title !== undefined ? { title: input.title } : {}),
      ...(input.excerpt !== undefined ? { excerpt: input.excerpt } : {}),
      ...(input.bodyMarkdown !== undefined ? { bodyMarkdown: input.bodyMarkdown } : {}),
      ...(input.status !== undefined ? { status: input.status } : {}),
    });
    const draft = await getContentDraft(this.database, draftId);
    if (!draft) throw new NotFoundException("draft not found");
    return draft;
  }

  async reviseDraft(draftId: string, instruction: string, scope: ContentRevisionScope): Promise<Record<string, unknown>> {
    const draft = await getContentDraft(this.database, draftId);
    if (!draft) throw new NotFoundException("draft not found");
    const project = await this.get(String(draft.project_id));
    const profile = (await this.profiles()).find((entry) => entry.key === draft.profile_key);
    if (!profile) throw new NotFoundException("platform profile not found");
    const document = await this.ai.reviseDraft({
      document: documentFromDraft(draft), instruction, brief: String(project.brief_markdown),
      profileName: String(profile.name), rules: platformProfileRulesSchema.parse(profile.rules),
      locale: draft.locale as ContentLocale,
    });
    await reviseContentDraft(this.database, { draftId, instruction, scope, document });
    return (await getContentDraft(this.database, draftId))!;
  }

  async publishBlog(projectId: string, input: {
    slug: string; authorName: string; seoTitle: string; seoDescription: string; relatedPaths: string[];
  }): Promise<Record<string, unknown>> {
    const project = await this.get(projectId);
    const blogDraft = (project.drafts as Array<Record<string, unknown>>).find((draft) => draft.profile_key === "blog");
    if (!blogDraft) throw new NotFoundException("generate the blog draft first");
    let post: Record<string, unknown>;
    try {
      post = await publishBlogPost(this.database, {
        projectId, draftId: String(blogDraft.id), canonicalUrl: `https://apitoken.sale/blog/${input.slug}`, ...input,
      });
    } catch (error) {
      if (error instanceof InvalidContentWorkflowError) throw new BadRequestException(error.message);
      if (isPostgresError(error, "23505")) throw new ConflictException("that blog slug is already in use");
      throw error;
    }
    void this.indexing.submitBlogPost(String(post.slug));
    return post;
  }

  async recordPublication(draftId: string, url: string): Promise<Record<string, unknown>> {
    const draft = await getContentDraft(this.database, draftId);
    if (!draft) throw new NotFoundException("draft not found");
    try {
      return await recordExternalPublication(this.database, { draftId, url });
    } catch (error) {
      if (error instanceof InvalidContentWorkflowError) throw new BadRequestException(error.message);
      if (isPostgresError(error, "23505")) throw new ConflictException("that publication URL or draft is already recorded");
      throw error;
    }
  }
}

function documentFromDraft(draft: Record<string, unknown>): DraftDocument {
  return { title: String(draft.title), excerpt: String(draft.excerpt), bodyMarkdown: String(draft.body_markdown) };
}

function isPostgresError(error: unknown, code: string): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === code;
}
