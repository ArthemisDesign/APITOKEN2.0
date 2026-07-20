import { ethers } from "ethers";

// On-chain слой пакетных выплат USDT в сети BNB (BEP-20). Отдельно: read-пул публичных RPC с
// фолбэком (баланс/нонс/чек-квитанции) и SEND-RPC (BlockRazor) для бродкаста. Приватный ключ горячего
// кошелька живёт только тут и НИКОГДА не логируется/не возвращается наружу.

// BEP-20 USDT: 18 знаков (не 6!). 1 USD = 1e9 nanoUSD = 1e18 usdt-wei → wei = nano * 1e9.
const NANO_TO_USDT_WEI = 1_000_000_000n;
const USDT_ABI = [
  "function transfer(address to, uint256 value) returns (bool)",
  "function balanceOf(address owner) view returns (uint256)",
  "function decimals() view returns (uint8)",
];
const TRANSFER_GAS_LIMIT = 100_000n;

export interface PayoutChainConfig {
  privateKey: string;
  sendRpcUrl: string;
  readRpcUrls: string[];
  usdtContract: string;
  chainId: number; // 56 = BSC mainnet
  gasPriceGwei: string; // напр. "0.05"
  confirmations: number;
}

export function nanoToUsdtWei(amountNano: bigint): bigint {
  return amountNano * NANO_TO_USDT_WEI;
}

/** Проверка адреса: валидный EVM + EIP-55 checksum. Возвращает checksummed адрес или бросает. */
export function normalizeBscAddress(address: string): string {
  const a = ethers.getAddress(address.trim()); // бросит на невалидном/битом checksum
  if (a === ethers.ZeroAddress) throw new Error("zero address");
  return a;
}

export interface SendResult { hash: string; nonce: number; }
export interface ConfirmResult { status: "confirmed" | "reverted"; blockNumber: number | null; }

export class PayoutChain {
  private readonly wallet: ethers.Wallet;
  private readonly sendProvider: ethers.JsonRpcProvider;
  private readonly readProviders: ethers.JsonRpcProvider[];
  private readonly usdt: string;
  private readonly gasPrice: bigint;
  private readonly confirmations: number;
  private readonly chainId: number;

  constructor(cfg: PayoutChainConfig) {
    if (!cfg.privateKey) throw new Error("payout hot-wallet key is not configured");
    if (!cfg.sendRpcUrl) throw new Error("payout send RPC is not configured");
    const net = ethers.Network.from(cfg.chainId);
    this.sendProvider = new ethers.JsonRpcProvider(cfg.sendRpcUrl, net, { staticNetwork: net });
    this.readProviders = (cfg.readRpcUrls.length ? cfg.readRpcUrls : [cfg.sendRpcUrl]).map(
      (url) => new ethers.JsonRpcProvider(url, net, { staticNetwork: net }),
    );
    this.wallet = new ethers.Wallet(cfg.privateKey);
    this.usdt = normalizeBscAddress(cfg.usdtContract);
    this.gasPrice = ethers.parseUnits(cfg.gasPriceGwei, "gwei");
    this.confirmations = Math.max(1, cfg.confirmations);
    this.chainId = cfg.chainId;
  }

  get hotAddress(): string {
    return this.wallet.address;
  }

  /** Перебирает read-RPC с фолбэком: первый ответивший выигрывает. */
  private async read<T>(fn: (p: ethers.JsonRpcProvider) => Promise<T>): Promise<T> {
    let lastErr: unknown;
    for (const p of this.readProviders) {
      try {
        return await fn(p);
      } catch (err) {
        lastErr = err;
      }
    }
    throw new Error(`all read RPCs failed: ${lastErr instanceof Error ? lastErr.message : "unknown"}`);
  }

  /** Проверяет, что SEND-RPC действительно сеть BNB (chainId совпал). */
  async assertNetwork(): Promise<void> {
    const net = await this.sendProvider.getNetwork();
    if (Number(net.chainId) !== this.chainId) {
      throw new Error(`send RPC chainId ${net.chainId} != expected ${this.chainId}`);
    }
  }

  async balances(): Promise<{ usdtWei: bigint; bnbWei: bigint }> {
    const usdtWei = await this.read((p) =>
      new ethers.Contract(this.usdt, USDT_ABI, p).getFunction("balanceOf").staticCall(this.hotAddress) as Promise<bigint>,
    );
    const bnbWei = await this.read((p) => p.getBalance(this.hotAddress));
    return { usdtWei, bnbWei };
  }

  gasCostPerTransferWei(): bigint {
    return TRANSFER_GAS_LIMIT * this.gasPrice;
  }

  async pendingNonce(): Promise<number> {
    return this.read((p) => p.getTransactionCount(this.hotAddress, "pending"));
  }

  /**
   * Симуляция (eth_call, staticCall) перевода: если бы транзакция ревёртнулась (нет баланса, битый
   * адрес и т.п.) — бросит ДО реальной отправки. Ничего не тратит и не отправляет.
   */
  async simulateTransfer(to: string, amountNano: bigint): Promise<void> {
    const recipient = normalizeBscAddress(to);
    const amountWei = nanoToUsdtWei(amountNano);
    await this.read((p) =>
      new ethers.Contract(this.usdt, USDT_ABI, p).getFunction("transfer").staticCall(recipient, amountWei, { from: this.hotAddress }),
    );
  }

  /** Бродкастит перевод через SEND-RPC (BlockRazor) с явными nonce/gasPrice/gasLimit. Возвращает хеш. */
  async sendTransfer(to: string, amountNano: bigint, nonce: number): Promise<SendResult> {
    const recipient = normalizeBscAddress(to);
    const amountWei = nanoToUsdtWei(amountNano);
    const contract = new ethers.Contract(this.usdt, USDT_ABI, this.wallet.connect(this.sendProvider));
    const tx = await contract.getFunction("transfer")(recipient, amountWei, {
      nonce,
      gasPrice: this.gasPrice,
      gasLimit: TRANSFER_GAS_LIMIT,
    }) as ethers.ContractTransactionResponse;
    return { hash: tx.hash, nonce };
  }

  /** Ждёт квитанцию (confirmations + timeout через read-пул). null → ещё не в блоке (таймаут). */
  async waitForConfirmation(txHash: string, timeoutMs = 90_000): Promise<ConfirmResult | null> {
    const receipt = await this.read((p) => p.waitForTransaction(txHash, this.confirmations, timeoutMs));
    if (!receipt) return null;
    return { status: receipt.status === 1 ? "confirmed" : "reverted", blockNumber: receipt.blockNumber ?? null };
  }

  /** Разовая проверка статуса без ожидания (для фонового поллера). */
  async checkReceipt(txHash: string): Promise<ConfirmResult | null> {
    const receipt = await this.read((p) => p.getTransactionReceipt(txHash));
    if (!receipt) return null;
    if (receipt.confirmations && (await receipt.confirmations()) < this.confirmations) return null;
    return { status: receipt.status === 1 ? "confirmed" : "reverted", blockNumber: receipt.blockNumber ?? null };
  }
}
