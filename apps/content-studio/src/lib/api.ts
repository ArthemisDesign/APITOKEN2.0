import type { ContentDraft, ContentProject, Locale, PlatformProfile } from "./types";

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !headers.has("content-type")) headers.set("content-type", "application/json");
  const response = await fetch(`/v1/admin/content${path}`, { ...init, headers, cache: "no-store" });
  if (!response.ok) {
    const payload = await response.json().catch(() => null) as { message?: string | string[] } | null;
    const message = Array.isArray(payload?.message) ? payload.message.join(". ") : payload?.message;
    throw new Error(message || `Request failed (${response.status})`);
  }
  return response.json() as Promise<T>;
}

export const studioApi = {
  status: () => request<{ aiEnabled: boolean; blogOrigin: string }>("/status"),
  projects: () => request<{ projects: ContentProject[] }>("/projects"),
  project: (id: string) => request<ContentProject>(`/projects/${id}`),
  importProject: (input: { sourceUrl: string; locale: Locale; sourceContent?: string }) =>
    request<ContentProject>("/projects/import", { method: "POST", body: JSON.stringify(input) }),
  updateProject: (id: string, input: Record<string, unknown>) =>
    request<ContentProject>(`/projects/${id}`, { method: "PATCH", body: JSON.stringify(input) }),
  generateBrief: (id: string) => request<ContentProject>(`/projects/${id}/brief/generate`, { method: "POST" }),
  profiles: () => request<{ profiles: PlatformProfile[] }>("/profiles"),
  saveProfile: (input: Record<string, unknown>) =>
    request<{ profiles: PlatformProfile[] }>("/profiles", { method: "POST", body: JSON.stringify(input) }),
  generateDrafts: (id: string, profiles: string[], locale: Locale) =>
    request<ContentProject>(`/projects/${id}/drafts/generate`, { method: "POST", body: JSON.stringify({ profiles, locale }) }),
  updateDraft: (id: string, input: Record<string, unknown>) =>
    request<ContentDraft>(`/drafts/${id}`, { method: "PATCH", body: JSON.stringify(input) }),
  reviseDraft: (id: string, instruction: string) =>
    request<ContentDraft>(`/drafts/${id}/revise`, { method: "POST", body: JSON.stringify({ instruction, scope: "draft" }) }),
  publishBlog: (id: string, input: Record<string, unknown>) =>
    request<Record<string, unknown>>(`/projects/${id}/blog/publish`, { method: "POST", body: JSON.stringify(input) }),
  recordPublication: (draftId: string, url: string) =>
    request<Record<string, unknown>>(`/drafts/${draftId}/publications`, { method: "POST", body: JSON.stringify({ url }) }),
};
