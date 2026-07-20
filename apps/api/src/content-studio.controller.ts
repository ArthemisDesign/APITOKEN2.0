import {
  BadRequestException,
  Body,
  ConflictException,
  Controller,
  Get,
  Header,
  Inject,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import {
  contentSourceSchema,
  generateContentDraftsSchema,
  importContentProjectSchema,
  publishBlogPostSchema,
  recordExternalPublicationSchema,
  reviseContentDraftSchema,
  updateContentDraftSchema,
  updateContentProjectSchema,
  upsertPlatformProfileSchema,
} from "@claude-api/contracts";
import {
  BlogFirstPublicationError,
  getPublishedBlogPost,
  listPublishedBlogPosts,
  type Database,
} from "@claude-api/db";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { ContentStudioService } from "./content-studio.service.js";
import { DATABASE } from "./infrastructure.module.js";

const uuidSchema = z.string().uuid();

@Controller("admin/content")
@UseGuards(AdminGuard)
export class ContentStudioController {
  constructor(private readonly studio: ContentStudioService) {}

  @Get("status")
  @Header("Cache-Control", "no-store")
  status(): unknown { return this.studio.status(); }

  @Get("projects")
  @Header("Cache-Control", "no-store")
  async list(): Promise<unknown> { return { projects: await this.studio.list() }; }

  @Get("projects/:id")
  @Header("Cache-Control", "no-store")
  async get(@Param("id") id: string): Promise<unknown> {
    return this.studio.get(uuid(id));
  }

  @Post("projects/import")
  @Header("Cache-Control", "no-store")
  async import(@Body() body: unknown): Promise<unknown> {
    const input = parse(importContentProjectSchema, body);
    return this.studio.import(input);
  }

  @Patch("projects/:id")
  @Header("Cache-Control", "no-store")
  async update(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    return this.studio.update(uuid(id), parse(updateContentProjectSchema, body));
  }

  @Post("projects/:id/sources")
  @Header("Cache-Control", "no-store")
  async addSource(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    return this.studio.addSource(uuid(id), parse(contentSourceSchema, body));
  }

  @Post("projects/:id/brief/generate")
  @Header("Cache-Control", "no-store")
  async generateBrief(@Param("id") id: string): Promise<unknown> {
    return this.studio.generateBrief(uuid(id));
  }

  @Get("profiles")
  @Header("Cache-Control", "no-store")
  async profiles(): Promise<unknown> { return { profiles: await this.studio.profiles() }; }

  @Post("profiles")
  @Header("Cache-Control", "no-store")
  async upsertProfile(@Body() body: unknown): Promise<unknown> {
    return { profiles: await this.studio.upsertProfile(parse(upsertPlatformProfileSchema, body)) };
  }

  @Post("projects/:id/drafts/generate")
  @Header("Cache-Control", "no-store")
  async generateDrafts(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    const input = parse(generateContentDraftsSchema, body);
    return this.studio.generateDrafts(uuid(id), input.profiles, input.locale);
  }

  @Patch("drafts/:id")
  @Header("Cache-Control", "no-store")
  async updateDraft(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    return this.studio.updateDraft(uuid(id), parse(updateContentDraftSchema, body));
  }

  @Post("drafts/:id/revise")
  @Header("Cache-Control", "no-store")
  async reviseDraft(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    const input = parse(reviseContentDraftSchema, body);
    return this.studio.reviseDraft(uuid(id), input.instruction, input.scope);
  }

  @Post("projects/:id/blog/publish")
  @Header("Cache-Control", "no-store")
  async publishBlog(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    return this.studio.publishBlog(uuid(id), parse(publishBlogPostSchema, body));
  }

  @Post("drafts/:id/publications")
  @Header("Cache-Control", "no-store")
  async recordPublication(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    try {
      return await this.studio.recordPublication(uuid(id), parse(recordExternalPublicationSchema, body).url);
    } catch (error) {
      if (error instanceof BlogFirstPublicationError) throw new ConflictException(error.message);
      throw error;
    }
  }
}

@Controller("blog")
export class PublicBlogController {
  constructor(@Inject(DATABASE) private readonly database: Database) {}

  @Get("posts")
  @Header("Cache-Control", "public, max-age=60, stale-while-revalidate=300")
  async list(@Query("limit") rawLimit?: string): Promise<unknown> {
    const parsedLimit = z.coerce.number().int().min(1).max(100).safeParse(rawLimit ?? 100);
    if (!parsedLimit.success) throw new BadRequestException("limit must be between 1 and 100");
    const limit = parsedLimit.data;
    return { posts: await listPublishedBlogPosts(this.database, limit) };
  }

  @Get("posts/:slug")
  @Header("Cache-Control", "public, max-age=60, stale-while-revalidate=300")
  async get(@Param("slug") slug: string, @Query("locale") rawLocale?: string): Promise<unknown> {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug)) throw new BadRequestException("invalid blog slug");
    const parsedLocale = z.enum(["en", "ru"]).safeParse(rawLocale ?? "en");
    if (!parsedLocale.success) throw new BadRequestException("locale must be en or ru");
    const locale = parsedLocale.data;
    const post = await getPublishedBlogPost(this.database, slug, locale);
    if (!post) throw new NotFoundException("blog post not found");
    return { post };
  }
}

function uuid(value: string): string {
  const parsed = uuidSchema.safeParse(value);
  if (!parsed.success) throw new BadRequestException("ID must be a UUID");
  return parsed.data;
}

function parse<S extends z.ZodType>(schema: S, body: unknown): z.output<S> {
  const parsed = schema.safeParse(body);
  if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
  return parsed.data as z.output<S>;
}
