import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import { clusterLabels, resolveArticle, type Locale } from "./learn";

export const OG_SIZE = { width: 1200, height: 630 };
export const OG_CONTENT_TYPE = "image/png";

async function font(file: string): Promise<Buffer> {
  return readFile(join(process.cwd(), "public/assets/fonts", file));
}

// JetBrains Mono covers Latin + Cyrillic, so this renders EN and RU titles.
export async function learnOgImage(slug: string, locale: Locale): Promise<ImageResponse> {
  const article = resolveArticle(slug, locale);
  const title = article?.content.h1 ?? "Claude API guides";
  const label = article ? clusterLabels[locale][article.cluster].label : "apiToken.sale";
  const [bold, regular] = await Promise.all([font("jetbrains-mono-700.ttf"), font("jetbrains-mono-400.ttf")]);

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          background: "linear-gradient(135deg, #304072 0%, #303E6A 100%)",
          padding: "72px",
          fontFamily: "JetBrains Mono",
          color: "#FFFFFF",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 16, fontSize: 30, fontWeight: 700, color: "#FFFFFF" }}>
          <div style={{ width: 22, height: 22, borderRadius: 6, background: "#C81D35" }} />
          apiToken.sale
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
          <div style={{ fontSize: 24, color: "#D6D6D6", textTransform: "uppercase", letterSpacing: 2 }}>{label}</div>
          <div style={{ fontSize: title.length > 46 ? 60 : 72, fontWeight: 700, lineHeight: 1.1, maxWidth: 1000 }}>{title}</div>
        </div>
        <div style={{ fontSize: 26, color: "#D6D6D6" }}>Claude API · flat 50% off · one key</div>
      </div>
    ),
    {
      ...OG_SIZE,
      fonts: [
        { name: "JetBrains Mono", data: bold, weight: 700, style: "normal" },
        { name: "JetBrains Mono", data: regular, weight: 400, style: "normal" },
      ],
    },
  );
}
