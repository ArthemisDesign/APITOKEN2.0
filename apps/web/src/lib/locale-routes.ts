export type CoreLocale = "en" | "ru";
export type DocumentLanguage = "en" | "ru" | "ko" | "zh-CN";

export function documentLanguageForPathname(pathname: string): DocumentLanguage {
  if (pathname === "/ru" || pathname.startsWith("/ru/")) return "ru";
  if (pathname === "/ko" || pathname.startsWith("/ko/")) return "ko";
  if (pathname === "/zh" || pathname.startsWith("/zh/")) return "zh-CN";
  return "en";
}

// Only routes with an actual Russian rendering belong here. Keeping this list
// explicit prevents a language control from manufacturing a /ru URL that the
// App Router cannot serve.
const russianExactRoutes = new Set([
  "/",
  "/dashboard",
  "/docs",
  "/docs/errors",
  "/forgot-password",
  "/integrations",
  "/int-claude-code",
  "/int-cline",
  "/int-codex",
  "/int-continue",
  "/int-cursor",
  "/int-opencode",
  "/int-sdk",
  "/int-zed",
  "/login",
  "/models",
  "/plans",
  "/privacy",
  "/register",
  "/reset-password",
  "/support",
  "/terms",
  "/verify-email",
]);

export function withoutRussianPrefix(pathname: string): string {
  if (pathname === "/ru") return "/";
  if (pathname.startsWith("/ru/")) return pathname.slice(3);
  return pathname;
}

export function supportsRussianRoute(pathname: string): boolean {
  const path = withoutRussianPrefix(pathname);
  return (
    russianExactRoutes.has(path) ||
    path.startsWith("/docs/learn/") ||
    path === "/docs/learn" ||
    path === "/errors" ||
    path.startsWith("/errors/")
  );
}

export function localeRoute(pathname: string, locale: CoreLocale): string | null {
  const path = withoutRussianPrefix(pathname);
  if (locale === "en") return path;
  if (!supportsRussianRoute(path)) return null;
  return path === "/" ? "/ru" : `/ru${path}`;
}

export function localeDestination(
  pathname: string,
  locale: CoreLocale,
  search = "",
  hash = "",
): string | null {
  const localized = localeRoute(pathname, locale);
  return localized ? `${localized}${search}${hash}` : null;
}

export function localeHref(href: string, locale: CoreLocale): string {
  if (!href.startsWith("/") || href.startsWith("//")) return href;
  const separatorIndex = href.search(/[?#]/);
  const pathname = separatorIndex < 0 ? href : href.slice(0, separatorIndex);
  const suffix = separatorIndex < 0 ? "" : href.slice(separatorIndex);
  const localized = localeRoute(pathname, locale);
  return localized ? `${localized}${suffix}` : href;
}

// The customer-facing landing is the static build in apps/web/public/landing
// (index.html — RU, en.html — EN), not the App Router "/" route. The brand
// logo in the app chrome must lead there, so /landing/* is deliberately kept
// out of russianExactRoutes: localeHref would otherwise rewrite the static
// path to a non-existent /ru/landing/* App Router URL.
export function landingHref(locale: CoreLocale): string {
  return locale === "ru" ? "/landing/index.html" : "/landing/en.html";
}

export const languagePreferenceBootstrapScript = `(()=>{try{if(localStorage.getItem('lang:v1')!=='ru')return;const p=location.pathname;if(${JSON.stringify([...russianExactRoutes])}.includes(p)||p==='/docs/learn'||p.startsWith('/docs/learn/')||p==='/errors'||p.startsWith('/errors/'))location.replace('/ru'+(p==='/'?'':p)+location.search+location.hash)}catch{}})()`;
