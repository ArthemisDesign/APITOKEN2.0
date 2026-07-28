import type { NextConfig } from "next";

const legacyPages = [
  "plans", "models", "docs", "login", "register", "dashboard", "integrations",
  "int-claude-code", "int-cursor", "int-cline", "int-continue", "int-zed", "int-sdk",
  "terms", "privacy",
];

const dashboardSections = [
  "overview", "keys", "credits", "promos", "usage", "profile", "security",
];

// Короткие ссылки для шеринга: /go/<slug> → главная с UTM. 302 (не permanent), чтобы можно было
// перенацелить slug и чтобы браузеры/мессенджеры не закешировали редирект навсегда.
const shortLinks: Record<string, string> = {
  vibe: "/?utm_source=telegram&utm_medium=community&utm_campaign=vibe_code_community",
};

const nextConfig: NextConfig = {
  poweredByHeader: false,
  async redirects() {
    return [
      ...legacyPages.map((page) => ({ source: `/${page}.html`, destination: `/${page}`, permanent: true })),
      ...dashboardSections.map((section) => ({ source: `/dashboard/${section}`, destination: "/dashboard", permanent: false })),
      ...Object.entries(shortLinks).map(([slug, destination]) => ({ source: `/go/${slug}`, destination, permanent: false })),
      // /e/<code> → справочник ошибок. Код едет ФРАГМЕНТОМ, а не query-параметром:
      // параметр породил бы второй сканируемый URL той же страницы (дубль), а фрагмент
      // поисковики отдельным URL не считают и браузер сам скроллит к секции.
      // Учёт по коду всё равно работает — ErrorAnchorBeacon читает location.hash
      // (Метрика срезает хеш только из отчётного URL, но не из того, что видит JS).
      { source: "/e/:code", destination: "/docs/errors#e-:code", permanent: false },
    ];
  },
  async headers() {
    return [{
      source: "/:path*",
      headers: [
        { key: "X-Content-Type-Options", value: "nosniff" },
        { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
        { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
      ],
    }];
  },
};

export default nextConfig;
