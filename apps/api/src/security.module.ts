import { Global, Module } from "@nestjs/common";
import { SecurityController } from "./security.controller.js";
import { TotpService } from "./totp.service.js";

// @Global → TotpService доступен и AccountController'у (гейт на выпуск ключа).
@Global()
@Module({
  controllers: [SecurityController],
  providers: [TotpService],
  exports: [TotpService],
})
export class SecurityModule {}
