import { Injectable, Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

@Injectable()
export class ContentIndexingService {
  private readonly logger = new Logger(ContentIndexingService.name);

  constructor(private readonly config: ConfigService<Environment, true>) {}

  async submitBlogPost(slug: string): Promise<void> {
    const key = this.config.get("INDEXNOW_KEY", { infer: true });
    const path = `/blog/${slug}`;
    try {
      const response = await fetch("https://api.indexnow.org/indexnow", {
        method: "POST",
        signal: AbortSignal.timeout(10_000),
        headers: { "content-type": "application/json; charset=utf-8" },
        body: JSON.stringify({
          host: "apitoken.sale",
          key,
          keyLocation: `https://apitoken.sale/${key}.txt`,
          urlList: [`https://apitoken.sale${path}`, "https://apitoken.sale/sitemap.xml", "https://apitoken.sale/feed.xml"],
        }),
      });
      if (!response.ok && response.status !== 202) this.logger.warn(`IndexNow returned ${response.status}`);
    } catch (error) {
      this.logger.warn(`IndexNow submission failed: ${error instanceof Error ? error.message : "unknown error"}`);
    }
  }
}
