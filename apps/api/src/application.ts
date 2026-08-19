import "reflect-metadata";
import helmet from "@fastify/helmet";
import { ConfigService } from "@nestjs/config";
import { NestFactory } from "@nestjs/core";
import { FastifyAdapter, type NestFastifyApplication } from "@nestjs/platform-fastify";
import { AppModule } from "./app.module.js";
import type { Environment } from "./config.js";

export async function createApplication(): Promise<NestFastifyApplication> {
  const app = await NestFactory.create<NestFastifyApplication>(AppModule, new FastifyAdapter({
    logger: false,
    bodyLimit: 1_048_576,
    // Единственный hop перед нами — Caddy на loopback; иначе request.ip навсегда 127.0.0.1,
    // а весь антифрод по IP слеп. Доверяем X-Forwarded-For только от loopback.
    trustProxy: ["127.0.0.1", "::1"],
  }), { rawBody: true });
  await app.register(helmet, { contentSecurityPolicy: false });
  const config = app.get(ConfigService<Environment, true>);
  // @fastify/cors по умолчанию отдаёт methods="GET,HEAD,POST" — без DELETE/PATCH браузер
  // заворачивает preflight на revoke-ключа и правку профиля ("Failed to fetch"). Перечисляем
  // явно все методы, которыми пользуется браузерный клиент.
  app.enableCors({
    origin: new URL(config.get("PUBLIC_APP_BASE_URL", { infer: true })).origin,
    credentials: true,
    methods: ["GET", "HEAD", "POST", "PATCH", "DELETE", "OPTIONS"],
  });
  app.setGlobalPrefix("v1");
  return app;
}
