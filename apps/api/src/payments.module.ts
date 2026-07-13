import { Global, Module } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { CryptomusProvider, DigiSellerProvider, PaymentProviderRegistry, type PaymentProviderAdapter } from "@claude-api/payments";
import type { Environment } from "./config.js";
import { CheckoutService } from "./checkout.service.js";
import { PaymentsController } from "./payments.controller.js";

@Global()
@Module({
  controllers: [PaymentsController],
  providers: [{
    provide: PaymentProviderRegistry,
    inject: [ConfigService],
    useFactory: (config: ConfigService<Environment, true>): PaymentProviderRegistry => {
      const adapters: PaymentProviderAdapter[] = [];
      const sellerId = config.get("DIGISELLER_SELLER_ID", { infer: true });
      const apiKey = config.get("DIGISELLER_API_KEY", { infer: true });
      const productId = config.get("DIGISELLER_PRODUCT_ID", { infer: true });
      const checkoutTrackingSecret = config.get("DIGISELLER_CHECKOUT_TRACKING_SECRET", { infer: true });
      if (sellerId !== undefined && apiKey !== undefined && productId !== undefined && checkoutTrackingSecret !== undefined) {
        adapters.push(new DigiSellerProvider({ sellerId, apiKey, productId, checkoutTrackingSecret }));
      }
      const cryptomusMerchantId = config.get("CRYPTOMUS_MERCHANT_ID", { infer: true });
      const cryptomusPaymentApiKey = config.get("CRYPTOMUS_PAYMENT_API_KEY", { infer: true });
      if (cryptomusMerchantId !== undefined && cryptomusPaymentApiKey !== undefined) {
        const publicApiBaseUrl = config.get("PUBLIC_API_BASE_URL", { infer: true });
        adapters.push(new CryptomusProvider({
          merchantId: cryptomusMerchantId,
          paymentApiKey: cryptomusPaymentApiKey,
          callbackUrl: new URL("/v1/payments/cryptomus/webhook", publicApiBaseUrl).toString(),
        }));
      }
      return new PaymentProviderRegistry(adapters);
    },
  }, CheckoutService],
  exports: [PaymentProviderRegistry],
})
export class PaymentsModule {}
