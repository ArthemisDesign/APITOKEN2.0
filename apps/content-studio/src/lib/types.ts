export type Locale = "en" | "ru";

export interface ContentDraft {
  id: string;
  project_id: string;
  profile_key: string;
  locale: Locale;
  title: string;
  excerpt: string;
  body_markdown: string;
  status: "draft" | "approved";
  revision: number;
}

export interface BlogPost {
  slug: string;
  status: "draft" | "published";
  published_at: string | null;
  locale: Locale;
}

export interface ContentProject {
  id: string;
  source_url: string;
  source_platform: string;
  source_title: string;
  source_author: string | null;
  source_content: string;
  primary_locale: Locale;
  status: string;
  brief_markdown: string;
  brief_version: number;
  updated_at: string;
  blog_slug?: string | null;
  blog_published_at?: string | null;
  draft_count?: number;
  publication_count?: number;
  drafts: ContentDraft[];
  sources: Array<{ id: string; url: string; title: string; source_type: string; notes: string }>;
  blog_post: BlogPost | null;
  publications: Array<{ id: string; draft_id: string; platform_key: string; url: string }>;
}

export interface PlatformProfile {
  id: string;
  key: string;
  name: string;
  rules: {
    language?: Locale;
    tone: string;
    audience: string;
    length: string;
    linkPolicy: string;
    requiredDisclosure: string;
    forbidden: string[];
  };
  built_in: boolean;
}
