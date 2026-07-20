import { describe, expect, it } from "vitest";
import { nanoToUsdtWei, normalizeBscAddress, PayoutChain } from "./chain.js";

const RECIPIENT = "0x55d398326f99059fF775485246999027B3197955";
function testChain(): PayoutChain {
  return new PayoutChain({
    privateKey: "0x" + "11".repeat(32),
    sendRpcUrl: "https://bsc.invalid",
    readRpcUrls: [],
    usdtContract: "0x55d398326f99059fF775485246999027B3197955",
    chainId: 56,
    gasPriceGwei: "0.1",
    confirmations: 1,
  });
}

describe("deterministic offline signing (double-pay safety)", () => {
  it("same (recipient, amount, nonce, gas) → SAME tx hash (re-broadcast is idempotent)", async () => {
    const chain = testChain();
    const a = await chain.signTransfer(RECIPIENT, 1_000_000_000n, 5);
    const b = await chain.signTransfer(RECIPIENT, 1_000_000_000n, 5);
    expect(a.hash).toBe(b.hash);
    expect(a.raw).toBe(b.raw);
    expect(a.hash).toMatch(/^0x[0-9a-f]{64}$/);
  });

  it("a different nonce → different hash (so a fresh nonce is never confused with the old tx)", async () => {
    const chain = testChain();
    const n5 = await chain.signTransfer(RECIPIENT, 1_000_000_000n, 5);
    const n6 = await chain.signTransfer(RECIPIENT, 1_000_000_000n, 6);
    expect(n6.hash).not.toBe(n5.hash);
  });
});

describe("payout chain helpers", () => {
  it("converts nanoUSD to USDT wei (BEP-20 has 18 decimals)", () => {
    expect(nanoToUsdtWei(1_000_000_000n)).toBe(1_000_000_000_000_000_000n); // $1 → 1e18 wei
    expect(nanoToUsdtWei(0n)).toBe(0n);
    expect(nanoToUsdtWei(1n)).toBe(1_000_000_000n); // 1 nano → 1e9 wei
    expect(nanoToUsdtWei(25_500_000_000n)).toBe(25_500_000_000_000_000_000n); // $25.5
  });

  it("accepts a valid checksummed BSC address and returns it checksummed", () => {
    const lower = "0x55d398326f99059ff775485246999027b3197955";
    const out = normalizeBscAddress(lower);
    expect(out).toBe("0x55d398326f99059fF775485246999027B3197955"); // ethers re-checksums
  });

  it("rejects a mistyped mixed-case address (bad EIP-55 checksum) — typo protection", () => {
    // correct: 0x55d398326f99059fF775485246999027B3197955; flip the "fF" → "Ff" → checksum no longer valid
    expect(() => normalizeBscAddress("0x55d398326f99059Ff775485246999027B3197955")).toThrow();
  });

  it("rejects malformed / zero / non-hex addresses", () => {
    expect(() => normalizeBscAddress("not-an-address")).toThrow();
    expect(() => normalizeBscAddress("0x123")).toThrow();
    expect(() => normalizeBscAddress("0x0000000000000000000000000000000000000000")).toThrow(); // zero address
  });
});
