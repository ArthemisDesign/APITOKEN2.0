export const dashboardSections = [
  "overview",
  "keys",
  "credits",
  "promos",
  "usage",
  "support",
  "profile",
  "security",
] as const;

export type DashboardSection = typeof dashboardSections[number];

export function parseDashboardSection(value: string | null): DashboardSection {
  return dashboardSections.includes(value as DashboardSection) ? value as DashboardSection : "overview";
}

export function dashboardHref(section: DashboardSection): string {
  return section === "overview" ? "/dashboard" : `/dashboard?view=${section}`;
}
