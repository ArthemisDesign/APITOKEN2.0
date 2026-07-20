CREATE TYPE "public"."blog_post_status" AS ENUM('draft', 'published');--> statement-breakpoint
CREATE TYPE "public"."content_draft_status" AS ENUM('draft', 'approved');--> statement-breakpoint
CREATE TYPE "public"."content_project_status" AS ENUM('imported', 'brief_ready', 'drafting', 'blog_published', 'distributed');--> statement-breakpoint
CREATE TABLE "blog_posts" (
	"id" uuid PRIMARY KEY NOT NULL,
	"project_id" uuid NOT NULL,
	"draft_id" uuid NOT NULL,
	"slug" text NOT NULL,
	"locale" text DEFAULT 'en' NOT NULL,
	"title" text NOT NULL,
	"excerpt" text NOT NULL,
	"body_markdown" text NOT NULL,
	"author_name" text DEFAULT 'apiToken.sale Editorial' NOT NULL,
	"seo_title" text NOT NULL,
	"seo_description" text NOT NULL,
	"source_urls" jsonb DEFAULT '[]'::jsonb NOT NULL,
	"related_paths" jsonb DEFAULT '[]'::jsonb NOT NULL,
	"status" "blog_post_status" DEFAULT 'draft' NOT NULL,
	"published_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "blog_posts_locale_check" CHECK ("blog_posts"."locale" IN ('en', 'ru')),
	CONSTRAINT "blog_posts_publication_check" CHECK (
    ("blog_posts"."status" = 'draft' AND "blog_posts"."published_at" IS NULL)
    OR ("blog_posts"."status" = 'published' AND "blog_posts"."published_at" IS NOT NULL)
  )
);
--> statement-breakpoint
CREATE TABLE "content_drafts" (
	"id" uuid PRIMARY KEY NOT NULL,
	"project_id" uuid NOT NULL,
	"profile_key" text NOT NULL,
	"locale" text DEFAULT 'en' NOT NULL,
	"title" text DEFAULT '' NOT NULL,
	"excerpt" text DEFAULT '' NOT NULL,
	"body_markdown" text DEFAULT '' NOT NULL,
	"status" "content_draft_status" DEFAULT 'draft' NOT NULL,
	"revision" integer DEFAULT 1 NOT NULL,
	"brief_version" integer DEFAULT 0 NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "content_drafts_locale_check" CHECK ("content_drafts"."locale" IN ('en', 'ru')),
	CONSTRAINT "content_drafts_revision_check" CHECK ("content_drafts"."revision" > 0),
	CONSTRAINT "content_drafts_brief_version_check" CHECK ("content_drafts"."brief_version" >= 0)
);
--> statement-breakpoint
CREATE TABLE "content_projects" (
	"id" uuid PRIMARY KEY NOT NULL,
	"source_url" text NOT NULL,
	"source_platform" text NOT NULL,
	"source_title" text DEFAULT '' NOT NULL,
	"source_author" text,
	"source_content" text DEFAULT '' NOT NULL,
	"source_published_at" timestamp with time zone,
	"source_snapshot" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"primary_locale" text DEFAULT 'en' NOT NULL,
	"status" "content_project_status" DEFAULT 'imported' NOT NULL,
	"brief_markdown" text DEFAULT '' NOT NULL,
	"brief_version" integer DEFAULT 0 NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "content_projects_locale_check" CHECK ("content_projects"."primary_locale" IN ('en', 'ru')),
	CONSTRAINT "content_projects_brief_version_check" CHECK ("content_projects"."brief_version" >= 0)
);
--> statement-breakpoint
CREATE TABLE "content_revisions" (
	"id" uuid PRIMARY KEY NOT NULL,
	"draft_id" uuid NOT NULL,
	"revision" integer NOT NULL,
	"scope" text NOT NULL,
	"instruction" text NOT NULL,
	"before" jsonb NOT NULL,
	"after" jsonb NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "content_revisions_scope_check" CHECK ("content_revisions"."scope" IN ('draft', 'platform', 'project', 'all')),
	CONSTRAINT "content_revisions_revision_check" CHECK ("content_revisions"."revision" > 1)
);
--> statement-breakpoint
CREATE TABLE "content_sources" (
	"id" uuid PRIMARY KEY NOT NULL,
	"project_id" uuid NOT NULL,
	"url" text NOT NULL,
	"title" text DEFAULT '' NOT NULL,
	"source_type" text DEFAULT 'reference' NOT NULL,
	"publisher" text,
	"published_at" timestamp with time zone,
	"notes" text DEFAULT '' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "external_publications" (
	"id" uuid PRIMARY KEY NOT NULL,
	"project_id" uuid NOT NULL,
	"draft_id" uuid NOT NULL,
	"platform_key" text NOT NULL,
	"url" text NOT NULL,
	"published_at" timestamp with time zone DEFAULT now() NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "external_publications_not_blog_check" CHECK ("external_publications"."platform_key" <> 'blog')
);
--> statement-breakpoint
CREATE TABLE "platform_profiles" (
	"id" uuid PRIMARY KEY NOT NULL,
	"key" text NOT NULL,
	"name" text NOT NULL,
	"rules" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"built_in" boolean DEFAULT false NOT NULL,
	"active" boolean DEFAULT true NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "blog_posts" ADD CONSTRAINT "blog_posts_project_id_content_projects_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."content_projects"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "blog_posts" ADD CONSTRAINT "blog_posts_draft_id_content_drafts_id_fk" FOREIGN KEY ("draft_id") REFERENCES "public"."content_drafts"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "content_drafts" ADD CONSTRAINT "content_drafts_project_id_content_projects_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."content_projects"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "content_revisions" ADD CONSTRAINT "content_revisions_draft_id_content_drafts_id_fk" FOREIGN KEY ("draft_id") REFERENCES "public"."content_drafts"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "content_sources" ADD CONSTRAINT "content_sources_project_id_content_projects_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."content_projects"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "external_publications" ADD CONSTRAINT "external_publications_project_id_content_projects_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."content_projects"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "external_publications" ADD CONSTRAINT "external_publications_draft_id_content_drafts_id_fk" FOREIGN KEY ("draft_id") REFERENCES "public"."content_drafts"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "blog_posts_project_uidx" ON "blog_posts" USING btree ("project_id");--> statement-breakpoint
CREATE UNIQUE INDEX "blog_posts_draft_uidx" ON "blog_posts" USING btree ("draft_id");--> statement-breakpoint
CREATE UNIQUE INDEX "blog_posts_slug_locale_uidx" ON "blog_posts" USING btree ("slug","locale");--> statement-breakpoint
CREATE INDEX "blog_posts_status_published_idx" ON "blog_posts" USING btree ("status","published_at");--> statement-breakpoint
CREATE UNIQUE INDEX "content_drafts_project_profile_locale_uidx" ON "content_drafts" USING btree ("project_id","profile_key","locale");--> statement-breakpoint
CREATE INDEX "content_drafts_project_idx" ON "content_drafts" USING btree ("project_id","updated_at");--> statement-breakpoint
CREATE INDEX "content_projects_status_updated_idx" ON "content_projects" USING btree ("status","updated_at");--> statement-breakpoint
CREATE UNIQUE INDEX "content_revisions_draft_revision_uidx" ON "content_revisions" USING btree ("draft_id","revision");--> statement-breakpoint
CREATE UNIQUE INDEX "content_sources_project_url_uidx" ON "content_sources" USING btree ("project_id","url");--> statement-breakpoint
CREATE INDEX "content_sources_project_idx" ON "content_sources" USING btree ("project_id","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "external_publications_draft_uidx" ON "external_publications" USING btree ("draft_id");--> statement-breakpoint
CREATE UNIQUE INDEX "external_publications_url_uidx" ON "external_publications" USING btree ("url");--> statement-breakpoint
CREATE INDEX "external_publications_project_idx" ON "external_publications" USING btree ("project_id","published_at");--> statement-breakpoint
CREATE UNIQUE INDEX "platform_profiles_key_uidx" ON "platform_profiles" USING btree ("key");--> statement-breakpoint
CREATE FUNCTION "enforce_blog_draft_identity"() RETURNS trigger AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1 FROM "content_drafts" AS draft
		WHERE draft."id" = NEW."draft_id"
		  AND draft."project_id" = NEW."project_id"
		  AND draft."profile_key" = 'blog'
		  AND draft."locale" = NEW."locale"
	) THEN
		RAISE EXCEPTION 'blog post must use the matching blog draft'
			USING ERRCODE = '23514', CONSTRAINT = 'blog_posts_blog_draft_required';
	END IF;
	RETURN NEW;
END;
$$ LANGUAGE plpgsql;--> statement-breakpoint
CREATE TRIGGER "blog_posts_blog_draft_required"
	BEFORE INSERT OR UPDATE OF "project_id", "draft_id", "locale"
	ON "blog_posts"
	FOR EACH ROW EXECUTE FUNCTION "enforce_blog_draft_identity"();--> statement-breakpoint
CREATE FUNCTION "enforce_blog_first_publication"() RETURNS trigger AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1 FROM "content_drafts" AS draft
		WHERE draft."id" = NEW."draft_id"
		  AND draft."project_id" = NEW."project_id"
		  AND draft."profile_key" = NEW."platform_key"
		  AND draft."profile_key" <> 'blog'
	) THEN
		RAISE EXCEPTION 'external publication must use its matching platform draft'
			USING ERRCODE = '23514', CONSTRAINT = 'external_publications_matching_draft_required';
	END IF;
	IF NOT EXISTS (
		SELECT 1 FROM "blog_posts" AS post
		WHERE post."project_id" = NEW."project_id"
		  AND post."status" = 'published'
		  AND post."published_at" IS NOT NULL
	) THEN
		RAISE EXCEPTION 'publish the canonical apiToken.sale blog post before external distribution'
			USING ERRCODE = '23514', CONSTRAINT = 'external_publications_blog_first_required';
	END IF;
	RETURN NEW;
END;
$$ LANGUAGE plpgsql;--> statement-breakpoint
CREATE TRIGGER "external_publications_blog_first_required"
	BEFORE INSERT OR UPDATE OF "project_id", "draft_id", "platform_key"
	ON "external_publications"
	FOR EACH ROW EXECUTE FUNCTION "enforce_blog_first_publication"();
