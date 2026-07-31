export const dashboardSections = [
  "overview",
  "keys",
  "credits",
  "promos",
  "usage",
  "support",
  "profile",
] as const;

export type DashboardSection = typeof dashboardSections[number];
export type DashboardLanguage = "en" | "ru";

export function parseDashboardSection(value: string | null): DashboardSection {
  // Keep old bookmarked URLs useful after Security was consolidated into Profile.
  if (value === "security") return "profile";
  return dashboardSections.includes(value as DashboardSection) ? value as DashboardSection : "overview";
}

export function dashboardHref(section: DashboardSection, language: DashboardLanguage = "en"): string {
  const base = language === "ru" ? "/ru/dashboard" : "/dashboard";
  return section === "overview" ? base : `${base}?view=${section}`;
}
