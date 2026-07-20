import { Module } from "@nestjs/common";
import { AdminGuard } from "./admin.guard.js";
import { ContentAiService } from "./content-ai.service.js";
import { ContentIndexingService } from "./content-indexing.service.js";
import { ContentSourceService } from "./content-source.service.js";
import { ContentStudioController, PublicBlogController } from "./content-studio.controller.js";
import { ContentStudioService } from "./content-studio.service.js";

@Module({
  controllers: [ContentStudioController, PublicBlogController],
  providers: [AdminGuard, ContentAiService, ContentIndexingService, ContentSourceService, ContentStudioService],
})
export class ContentStudioModule {}
