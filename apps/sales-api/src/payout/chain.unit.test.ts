import { describe, expect, it, vi } from "vitest";
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

describe("retained transaction reconciliation", () => {
  function provider(overrides: Record<string, unknown> = {}) {
    return {
      getTransactionReceipt: vi.fn().mockResolvedValue(null),
      getTransaction: vi.fn().mockResolvedValue(null),
      getTransactionCount: vi.fn().mockResolvedValue(8),
      ...overrides,
    };
  }

  function withProviders(...providers: ReturnType<typeof provider>[]): PayoutChain {
    const chain = testChain();
    (chain as unknown as { readProviders: unknown[] }).readProviders = providers;
    return chain;
  }

  it("accepts an authoritative confirmed receipt before considering nonce evidence", async () => {
    const receipt = { status: 1, blockNumber: 100, confirmations: vi.fn().mockResolvedValue(3) };
    const chain = withProviders(provider({ getTransactionReceipt: vi.fn().mockResolvedValue(receipt) }));
    await expect(chain.reconcileTransaction(`0x${"1".repeat(64)}`, 7)).resolves.toEqual({
      status: "confirmed",
      blockNumber: 100,
    });
  });

  it("requires every read RPC to agree before declaring a nonce consumed elsewhere", async () => {
    const txHash = `0x${"2".repeat(64)}`;
    await expect(withProviders(provider(), provider()).reconcileTransaction(txHash, 7)).resolves.toEqual({
      status: "nonce_consumed",
      blockNumber: null,
    });
    await expect(withProviders(
      provider(),
      provider({ getTransactionCount: vi.fn().mockResolvedValue(7) }),
    ).reconcileTransaction(txHash, 7)).resolves.toBeNull();
  });

  it("fails closed on a pending hash, provider error, missing nonce or immature receipt", async () => {
    const txHash = `0x${"3".repeat(64)}`;
    await expect(withProviders(provider({
      getTransaction: vi.fn().mockResolvedValue({ hash: txHash }),
    })).reconcileTransaction(txHash, 7)).resolves.toBeNull();
    await expect(withProviders(provider({
      getTransactionReceipt: vi.fn().mockRejectedValue(new Error("offline")),
    })).reconcileTransaction(txHash, 7)).resolves.toBeNull();
    await expect(withProviders(provider()).reconcileTransaction(txHash, null)).resolves.toBeNull();
    await expect(withProviders(provider({
      getTransactionReceipt: vi.fn().mockResolvedValue({
        status: 1,
        blockNumber: 100,
        confirmations: vi.fn().mockResolvedValue(0),
      }),
    })).reconcileTransaction(txHash, 7)).resolves.toBeNull();
  });
});
