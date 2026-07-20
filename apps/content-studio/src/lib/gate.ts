import type { ContentProject } from "./types";

export function canPublishExternally(project: Pick<ContentProject, "blog_post"> | null): boolean {
  return project?.blog_post?.status === "published" && Boolean(project.blog_post.published_at);
}

export function slugify(value: string): string {
  return value.toLowerCase().normalize("NFKD").replace(/[^a-z0-9\s-]/g, "")
    .trim().replace(/[\s-]+/g, "-").slice(0, 120);
}
