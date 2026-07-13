import { Global, Module } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { APP_GUARD } from "@nestjs/core";
import { AuthController } from "./auth.controller.js";
import { SessionAuthGuard } from "./auth.guard.js";
import { AuthService } from "./auth.service.js";
import { OriginGuard } from "./origin.guard.js";
import type { Environment } from "./config.js";
import { GitHubIdentityProvider, GoogleIdentityProvider, OAuthProviderRegistry, type ExternalIdentityProvider } from "./auth-providers.js";

@Global()
@Module({
  controllers: [AuthController],
  providers: [
    {
      provide: OAuthProviderRegistry,
      inject: [ConfigService],
      useFactory: (config: ConfigService<Environment, true>) => {
        const providers: ExternalIdentityProvider[] = [];
        const googleClientId = config.get("GOOGLE_CLIENT_ID", { infer: true });
        const googleClientSecret = config.get("GOOGLE_CLIENT_SECRET", { infer: true });
        const googleRedirectUri = config.get("GOOGLE_REDIRECT_URI", { infer: true });
        if (googleClientId && googleClientSecret && googleRedirectUri) {
          providers.push(new GoogleIdentityProvider({ clientId: googleClientId, clientSecret: googleClientSecret, redirectUri: googleRedirectUri }));
        }
        const githubClientId = config.get("GITHUB_CLIENT_ID", { infer: true });
        const githubClientSecret = config.get("GITHUB_CLIENT_SECRET", { infer: true });
        const githubRedirectUri = config.get("GITHUB_REDIRECT_URI", { infer: true });
        if (githubClientId && githubClientSecret && githubRedirectUri) {
          providers.push(new GitHubIdentityProvider({ clientId: githubClientId, clientSecret: githubClientSecret, redirectUri: githubRedirectUri }));
        }
        return new OAuthProviderRegistry(providers);
      },
    },
    AuthService,
    SessionAuthGuard,
    { provide: APP_GUARD, useClass: OriginGuard },
  ],
  exports: [AuthService, SessionAuthGuard],
})
export class AuthModule {}
