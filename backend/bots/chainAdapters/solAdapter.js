import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import { Connection, Keypair, PublicKey, Transaction, SystemProgram } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';

class SolAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, rpcUrl }) {
    super({ walletPrivateKey, rpcUrl });
  }

  async initialize() {
    this.provider = new Connection(this.rpcUrl, 'confirmed');
    this.wallet = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(this.walletPrivateKey)));
    await logger(`Solana Adapter khởi tạo với ví: ${this.wallet.publicKey.toBase58()}`);
  }

  async watchMempool(callback) {
    this.provider.onPendingTransaction((tx) => callback(tx));
  }

  async getFeeEstimate() {
    const recentBlockhash = await this.provider.getRecentBlockhash();
    return recentBlockhash.feeCalculator.lamportsPerSignature;
  }

  async buyToken(tokenAddress, amount) {
    const fee = await this.getFeeEstimate();
    const tx = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: this.wallet.publicKey,
        toPubkey: new PublicKey('RAYDIUM_POOL_ADDRESS'), // Raydium pool
        lamports: amount * 1e9 + fee // amount + phí
      })
    );
    const result = await retry(() => this.provider.sendTransaction(tx, [this.wallet]));
    await logger(`Mua token ${tokenAddress} với ${amount} SOL`);
    return result;
  }

  async sellToken(tokenAddress, amount) {
    const fee = await this.getFeeEstimate();
    const tx = new Transaction().add(
      TOKEN_PROGRAM_ID.transfer({
        fromPubkey: this.wallet.publicKey,
        toPubkey: new PublicKey('RAYDIUM_POOL_ADDRESS'),
        amount: amount * 1e9,
        fee
      })
    );
    const result = await retry(() => this.provider.sendTransaction(tx, [this.wallet]));
    await logger(`Bán token ${tokenAddress} với ${amount} token`);
    return result;
  }
}

export default SolAdapter;