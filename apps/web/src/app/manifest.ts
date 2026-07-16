import type { MetadataRoute } from "next";
import { SITE_NAME } from "@/lib/seo";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: `${SITE_NAME} — Claude API Access`,
    short_name: SITE_NAME,
    description: "One API key and prepaid balance for supported Claude models.",
    start_url: "/",
    display: "standalone",
    background_color: "#0a0a0a",
    theme_color: "#0a0a0a",
    lang: "en",
    categories: ["developer tools", "productivity", "utilities"],
    icons: [
      { src: "/assets/favicon-192.png", sizes: "192x192", type: "image/png" },
      { src: "/assets/favicon-512.png", sizes: "512x512", type: "image/png" },
      { src: "/assets/favicon-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
