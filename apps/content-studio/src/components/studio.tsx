"use client";

import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { studioApi } from "@/lib/api";
import { canPublishExternally, slugify } from "@/lib/gate";
import type { ContentDraft, ContentProject, Locale, PlatformProfile } from "@/lib/types";

type BusyAction = "import" | "brief" | "drafts" | "save" | "revise" | "publish" | "record" | "profile" | null;

export function Studio() {
  const [projects, setProjects] = useState<ContentProject[]>([]);
  const [profiles, setProfiles] = useState<PlatformProfile[]>([]);
  const [project, setProject] = useState<ContentProject | null>(null);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [aiEnabled, setAiEnabled] = useState(false);
  const [busy, setBusy] = useState<BusyAction>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const activeDraft = useMemo(
    () => project?.drafts.find((draft) => draft.id === activeDraftId) ?? project?.drafts[0] ?? null,
    [activeDraftId, project],
  );

  const refreshProject = useCallback(async (id: string) => {
    const next = await studioApi.project(id);
    setProject(next);
    setActiveDraftId((current) => current && next.drafts.some((draft) => draft.id === current)
      ? current : next.drafts[0]?.id ?? null);
    const listing = await studioApi.projects();
    setProjects(listing.projects);
  }, []);

  useEffect(() => {
    void Promise.all([studioApi.status(), studioApi.projects(), studioApi.profiles()])
      .then(([status, projectList, profileList]) => {
        setAiEnabled(status.aiEnabled);
        setProjects(projectList.projects);
        setProfiles(profileList.profiles);
        if (projectList.projects[0]) return refreshProject(projectList.projects[0].id);
      })
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, [refreshProject]);

  const run = async (action: BusyAction, operation: () => Promise<void>, success: string) => {
    setBusy(action); setError(""); setMessage("");
    try { await operation(); setMessage(success); }
    catch (cause) { setError(errorMessage(cause)); }
    finally { setBusy(null); }
  };

  const selectProject = (id: string) => void run("save", async () => refreshProject(id), "Project loaded.");

  return (
    <main className="shell">
      <header className="topbar">
        <div><p className="eyebrow">apiToken.sale · private workspace</p><h1>Content Studio</h1></div>
        <div className={`engine ${aiEnabled ? "ready" : "offline"}`}><span aria-hidden="true" /> AI {aiEnabled ? "connected" : "needs an API key"}</div>
      </header>
      {(message || error) && <div className={`notice ${error ? "notice-error" : "notice-ok"}`} role="status">{error || message}</div>}
      <div className="workspace">
        <aside className="rail">
          <ImportForm busy={busy === "import"} onImport={(input) => run("import", async () => {
            const created = await studioApi.importProject(input); await refreshProject(created.id);
          }, "Source imported. Verify it before generating content.")} />
          <section className="project-list" aria-label="Content projects">
            <div className="section-heading"><h2>Projects</h2><span>{projects.length}</span></div>
            {projects.length === 0 && <p className="muted">Your imported sources will appear here.</p>}
            {projects.map((item) => <button key={item.id} className={`project-card ${item.id === project?.id ? "selected" : ""}`} onClick={() => selectProject(item.id)}>
              <span className="platform">{item.source_platform}</span><strong>{item.source_title || item.source_url}</strong><small>{item.blog_published_at ? "Canonical live" : `${item.draft_count ?? 0} drafts`}</small>
            </button>)}
          </section>
        </aside>
        <section className="canvas">
          {!project ? <EmptyState /> : <>
            <SourceAndBrief project={project} aiEnabled={aiEnabled} busy={busy} onSave={(input) => run("save", async () => {
              await studioApi.updateProject(project.id, input); await refreshProject(project.id);
            }, "Source and brief saved.")} onGenerate={() => run("brief", async () => {
              await studioApi.generateBrief(project.id); await refreshProject(project.id);
            }, "Verification brief generated. Review it before drafting.")} />
            <DraftGenerator profiles={profiles} locale={project.primary_locale} aiEnabled={aiEnabled} busy={busy === "drafts"} onGenerate={(selected, locale) => run("drafts", async () => {
              await studioApi.generateDrafts(project.id, selected, locale); await refreshProject(project.id);
            }, "Independent platform drafts generated.")} onCreateProfile={(input) => run("profile", async () => {
              const result = await studioApi.saveProfile(input); setProfiles(result.profiles);
            }, "Custom platform profile saved.")} />
            <DraftWorkspace project={project} profiles={profiles} activeDraft={activeDraft} setActiveDraftId={setActiveDraftId} busy={busy} aiEnabled={aiEnabled}
              onSave={(draft, input) => run("save", async () => { await studioApi.updateDraft(draft.id, input); await refreshProject(project.id); }, `${profileName(profiles, draft.profile_key)} draft saved.`)}
              onRevise={(draft, instruction) => run("revise", async () => { await studioApi.reviseDraft(draft.id, instruction); await refreshProject(project.id); }, `Only the ${profileName(profiles, draft.profile_key)} draft was revised.`)}
              onPublish={(input) => run("publish", async () => { await studioApi.publishBlog(project.id, input); await refreshProject(project.id); }, "Canonical blog article published. External distribution is now unlocked.")}
              onRecord={(draft, url) => run("record", async () => { await studioApi.recordPublication(draft.id, url); await refreshProject(project.id); }, `${profileName(profiles, draft.profile_key)} publication linked to the canonical article.`)} />
          </>}
        </section>
      </div>
    </main>
  );
}

function ImportForm({ busy, onImport }: { busy: boolean; onImport: (input: { sourceUrl: string; locale: Locale; sourceContent?: string }) => void }) {
  const [sourceUrl, setSourceUrl] = useState(""); const [locale, setLocale] = useState<Locale>("en"); const [sourceContent, setSourceContent] = useState("");
  const submit = (event: FormEvent) => { event.preventDefault(); onImport({ sourceUrl, locale, ...(sourceContent.trim() ? { sourceContent } : {}) }); };
  return <form className="card import-card" onSubmit={submit}>
    <p className="step">01 · Capture</p><h2>Import a source</h2>
    <label>Social or article URL<input type="url" required placeholder="https://x.com/..." value={sourceUrl} onChange={(event) => setSourceUrl(event.target.value)} /></label>
    <label>Writing language<select value={locale} onChange={(event) => setLocale(event.target.value as Locale)}><option value="en">English</option><option value="ru">Russian</option></select></label>
    <label>Manual source text <span>(optional)</span><textarea rows={4} placeholder="Paste text when a platform blocks extraction." value={sourceContent} onChange={(event) => setSourceContent(event.target.value)} /></label>
    <button className="primary full" disabled={busy}>{busy ? "Importing…" : "Import source"}</button>
  </form>;
}

function SourceAndBrief({ project, aiEnabled, busy, onSave, onGenerate }: { project: ContentProject; aiEnabled: boolean; busy: BusyAction; onSave: (input: Record<string, unknown>) => void; onGenerate: () => void }) {
  const [sourceTitle, setSourceTitle] = useState(project.source_title); const [sourceAuthor, setSourceAuthor] = useState(project.source_author ?? ""); const [sourceContent, setSourceContent] = useState(project.source_content); const [briefMarkdown, setBriefMarkdown] = useState(project.brief_markdown);
  useEffect(() => { setSourceTitle(project.source_title); setSourceAuthor(project.source_author ?? ""); setSourceContent(project.source_content); setBriefMarkdown(project.brief_markdown); }, [project]);
  return <section className="card stage">
    <div className="stage-head"><div><p className="step">02 · Verify</p><h2>Source and factual brief</h2></div><a href={project.source_url} target="_blank" rel="noreferrer">Open source ↗</a></div>
    <div className="two-col"><div><label>Source title<input value={sourceTitle} onChange={(e) => setSourceTitle(e.target.value)} /></label><label>Source author<input value={sourceAuthor} onChange={(e) => setSourceAuthor(e.target.value)} /></label><label>Captured source text<textarea rows={12} value={sourceContent} onChange={(e) => setSourceContent(e.target.value)} /></label></div><div><label>Verified brief <span>version {project.brief_version}</span><textarea className="brief" rows={17} value={briefMarkdown} onChange={(e) => setBriefMarkdown(e.target.value)} placeholder="Generate, then fact-check this brief." /></label></div></div>
    <div className="actions"><button className="secondary" disabled={busy !== null} onClick={() => onSave({ sourceTitle, sourceAuthor: sourceAuthor || null, sourceContent, briefMarkdown })}>Save edits</button><button className="primary" disabled={busy !== null || !aiEnabled} title={!aiEnabled ? "Configure CONTENT_STUDIO_ENGINE_KEY first" : ""} onClick={onGenerate}>{busy === "brief" ? "Researching…" : project.brief_markdown ? "Regenerate brief" : "Generate verified brief"}</button></div>
  </section>;
}

function DraftGenerator({ profiles, locale, aiEnabled, busy, onGenerate, onCreateProfile }: { profiles: PlatformProfile[]; locale: Locale; aiEnabled: boolean; busy: boolean; onGenerate: (profiles: string[], locale: Locale) => void; onCreateProfile: (input: Record<string, unknown>) => void }) {
  const [selected, setSelected] = useState(["blog", "reddit", "vc-ru", "dzen"]); const [draftLocale, setDraftLocale] = useState(locale); const [showProfile, setShowProfile] = useState(false);
  const toggle = (key: string) => setSelected((current) => current.includes(key) ? current.filter((item) => item !== key) : [...current, key]);
  return <section className="card stage">
    <div className="stage-head"><div><p className="step">03 · Adapt</p><h2>Generate independent drafts</h2></div><button className="text-button" onClick={() => setShowProfile(!showProfile)}>+ Custom platform</button></div>
    <div className="profile-grid">{profiles.map((profile) => <label className={`profile-chip ${selected.includes(profile.key) ? "active" : ""}`} key={profile.key}><input type="checkbox" checked={selected.includes(profile.key)} onChange={() => toggle(profile.key)} /><strong>{profile.name}</strong><span>{profile.rules.length}</span></label>)}</div>
    {showProfile && <CustomProfile onSave={(input) => { onCreateProfile(input); setShowProfile(false); }} />}
    <div className="actions"><label className="inline-label">Language<select value={draftLocale} onChange={(e) => setDraftLocale(e.target.value as Locale)}><option value="en">English</option><option value="ru">Russian</option></select></label><button className="primary" disabled={busy || !aiEnabled || selected.length === 0} onClick={() => onGenerate(selected, draftLocale)}>{busy ? "Writing…" : `Generate ${selected.length} drafts`}</button></div>
  </section>;
}

function CustomProfile({ onSave }: { onSave: (input: Record<string, unknown>) => void }) {
  const [name, setName] = useState(""); const [key, setKey] = useState(""); const [tone, setTone] = useState(""); const [audience, setAudience] = useState(""); const [length, setLength] = useState(""); const [linkPolicy, setLinkPolicy] = useState("");
  return <div className="custom-profile"><h3>New platform rules</h3><div className="three-col"><label>Name<input value={name} onChange={(e) => { setName(e.target.value); if (!key) setKey(slugify(e.target.value)); }} /></label><label>Key<input value={key} onChange={(e) => setKey(slugify(e.target.value))} /></label><label>Length<input placeholder="500–900 words" value={length} onChange={(e) => setLength(e.target.value)} /></label></div><div className="two-col"><label>Tone<input value={tone} onChange={(e) => setTone(e.target.value)} /></label><label>Audience<input value={audience} onChange={(e) => setAudience(e.target.value)} /></label></div><label>Link policy<input value={linkPolicy} onChange={(e) => setLinkPolicy(e.target.value)} /></label><button className="secondary" disabled={!name || !key || !tone || !audience || !length || !linkPolicy} onClick={() => onSave({ key, name, rules: { tone, audience, length, linkPolicy, requiredDisclosure: "", forbidden: [] } })}>Save platform</button></div>;
}

function DraftWorkspace({ project, profiles, activeDraft, setActiveDraftId, busy, aiEnabled, onSave, onRevise, onPublish, onRecord }: { project: ContentProject; profiles: PlatformProfile[]; activeDraft: ContentDraft | null; setActiveDraftId: (id: string) => void; busy: BusyAction; aiEnabled: boolean; onSave: (draft: ContentDraft, input: Record<string, unknown>) => void; onRevise: (draft: ContentDraft, instruction: string) => void; onPublish: (input: Record<string, unknown>) => void; onRecord: (draft: ContentDraft, url: string) => void }) {
  return <section className="card stage"><p className="step">04 · Edit and publish</p><h2>Platform workbench</h2>
    {project.drafts.length === 0 ? <p className="muted roomy">Generate drafts after the brief is verified.</p> : <><div className="tabs" role="tablist">{project.drafts.map((draft) => <button role="tab" aria-selected={activeDraft?.id === draft.id} className={activeDraft?.id === draft.id ? "active" : ""} key={draft.id} onClick={() => setActiveDraftId(draft.id)}>{profileName(profiles, draft.profile_key)} <small>v{draft.revision}</small></button>)}</div>{activeDraft && <DraftEditor key={`${activeDraft.id}-${activeDraft.revision}`} project={project} draft={activeDraft} profile={profileName(profiles, activeDraft.profile_key)} busy={busy} aiEnabled={aiEnabled} onSave={onSave} onRevise={onRevise} onPublish={onPublish} onRecord={onRecord} />}</>}
  </section>;
}

function DraftEditor({ project, draft, profile, busy, aiEnabled, onSave, onRevise, onPublish, onRecord }: { project: ContentProject; draft: ContentDraft; profile: string; busy: BusyAction; aiEnabled: boolean; onSave: (draft: ContentDraft, input: Record<string, unknown>) => void; onRevise: (draft: ContentDraft, instruction: string) => void; onPublish: (input: Record<string, unknown>) => void; onRecord: (draft: ContentDraft, url: string) => void }) {
  const [title, setTitle] = useState(draft.title); const [excerpt, setExcerpt] = useState(draft.excerpt); const [bodyMarkdown, setBody] = useState(draft.body_markdown); const [instruction, setInstruction] = useState(""); const [publicationUrl, setPublicationUrl] = useState("");
  const [slug, setSlug] = useState(slugify(draft.title)); const [authorName, setAuthorName] = useState("apiToken.sale Editorial"); const [seoTitle, setSeoTitle] = useState(draft.title.slice(0, 70)); const [seoDescription, setSeoDescription] = useState(draft.excerpt.slice(0, 170)); const [relatedPaths, setRelatedPaths] = useState("");
  const unlocked = canPublishExternally(project); const alreadyRecorded = project.publications.some((publication) => publication.draft_id === draft.id);
  return <div className="editor">
    <div className="editor-meta"><span>{profile}</span><span>{draft.locale.toUpperCase()}</span><span>Revision {draft.revision}</span></div>
    <label>Title<input value={title} onChange={(e) => setTitle(e.target.value)} /></label><label>Excerpt<textarea rows={3} value={excerpt} onChange={(e) => setExcerpt(e.target.value)} /></label><label>Body in Markdown<textarea className="draft-body" rows={22} value={bodyMarkdown} onChange={(e) => setBody(e.target.value)} /></label>
    <div className="actions"><button className="secondary" disabled={busy !== null} onClick={() => onSave(draft, { title, excerpt, bodyMarkdown })}>Save {profile}</button></div>
    <div className="ai-revision"><div><strong>Ask AI to edit only this draft</strong><p>The brief and every other platform draft stay unchanged.</p></div><textarea rows={3} placeholder={`Example: Make this ${profile} version more technical and move the evidence first.`} value={instruction} onChange={(e) => setInstruction(e.target.value)} /><button className="primary" disabled={busy !== null || !aiEnabled || instruction.trim().length < 3} onClick={() => onRevise(draft, instruction)}>{busy === "revise" ? "Revising…" : `Revise only ${profile}`}</button></div>
    {draft.profile_key === "blog" ? <div className="publish-box"><div className="lock-line unlocked"><span>✓</span><div><strong>Canonical article</strong><p>This must be published before any external version.</p></div></div><div className="two-col"><label>SEO slug<input value={slug} onChange={(e) => setSlug(slugify(e.target.value))} /></label><label>Author<input value={authorName} onChange={(e) => setAuthorName(e.target.value)} /></label></div><label>SEO title <span>{seoTitle.length}/70</span><input value={seoTitle} onChange={(e) => setSeoTitle(e.target.value.slice(0, 70))} /></label><label>Meta description <span>{seoDescription.length}/170</span><textarea rows={2} value={seoDescription} onChange={(e) => setSeoDescription(e.target.value.slice(0, 170))} /></label><label>Related apiToken.sale paths <span>comma-separated</span><input placeholder="/docs/learn/..., /models/..." value={relatedPaths} onChange={(e) => setRelatedPaths(e.target.value)} /></label>{project.blog_post?.status === "published" && <a className="canonical-link" target="_blank" rel="noreferrer" href={`https://apitoken.sale/blog/${project.blog_post.slug}`}>View canonical article ↗</a>}<button className="publish" disabled={busy !== null || !slug || !seoTitle || !seoDescription} onClick={() => onPublish({ slug, authorName, seoTitle, seoDescription, relatedPaths: relatedPaths.split(",").map((item) => item.trim()).filter(Boolean) })}>{busy === "publish" ? "Publishing…" : project.blog_post?.status === "published" ? "Update published article" : "Publish canonical article"}</button></div>
      : <div className={`publish-box ${unlocked ? "" : "locked"}`}><div className={`lock-line ${unlocked ? "unlocked" : ""}`}><span>{unlocked ? "✓" : "🔒"}</span><div><strong>{unlocked ? "External publishing unlocked" : "External publishing locked"}</strong><p>{unlocked ? `Publish on ${profile}, then save its live URL here.` : "Publish the apiToken.sale blog article first. The API and database enforce this too."}</p></div></div>{alreadyRecorded ? <p className="recorded">✓ Publication URL recorded</p> : <div className="publication-row"><input type="url" disabled={!unlocked} placeholder={`Live ${profile} post URL`} value={publicationUrl} onChange={(e) => setPublicationUrl(e.target.value)} /><button className="publish" disabled={!unlocked || busy !== null || !publicationUrl} onClick={() => onRecord(draft, publicationUrl)}>{busy === "record" ? "Saving…" : "Record external post"}</button></div>}</div>}
  </div>;
}

function EmptyState() { return <div className="empty"><span>✦</span><h2>Turn one source into a verified content system</h2><p>Import a social post or article. The studio will preserve the source, build a factual brief, and create platform-specific drafts without mixing their edits.</p></div>; }
function profileName(profiles: PlatformProfile[], key: string): string { return profiles.find((profile) => profile.key === key)?.name ?? key; }
function errorMessage(cause: unknown): string { return cause instanceof Error ? cause.message : "Something went wrong"; }
