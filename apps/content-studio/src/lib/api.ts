import type { ContentDraft, ContentProject, Locale, PlatformProfile } from "./types";

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !headers.has("content-type")) headers.set("content-type", "application/json");
  const response = await fetch(`/v1/admin/content${path}`, { ...init, headers, cache: "no-store" });
  if (!response.ok) {
    const payload = await response.json().catch(() => null) as { message?: unknown } | null;
    throw new Error(apiErrorMessage(payload, response.status));
  }
  return response.json() as Promise<T>;
}

export function apiErrorMessage(payload: { message?: unknown } | null, status: number): string {
  const message = payload?.message;
  if (typeof message === "string" && message.trim()) return message;
  if (Array.isArray(message)) {
    const values = message.filter((item): item is string => typeof item === "string" && Boolean(item.trim()));
    if (values.length > 0) return values.join(". ");
  }
  if (isRecord(message)) {
    const formErrors = stringArray(message.formErrors);
    const fieldErrors = isRecord(message.fieldErrors)
      ? Object.entries(message.fieldErrors).flatMap(([field, errors]) =>
        stringArray(errors).map((error) => `${field}: ${error}`))
      : [];
    const details = [...formErrors, ...fieldErrors];
    if (details.length > 0) return details.join(". ");
  }
  return `Request failed (${status})`;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string" && Boolean(item.trim()))
    : [];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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
