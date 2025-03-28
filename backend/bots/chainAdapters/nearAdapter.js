import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import * as nearAPI from 'near-api-js';
import config from '../../config.js';

class NearAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, rpcUrl, networkId = 'testnet' }) {
    super({ walletPrivateKey, rpcUrl });
    this.networkId = networkId; // mainnet hoặc testnet
  }

  async initialize() {
    const { keyStores, connect } = nearAPI;
    const keyStore = new keyStores.InMemoryKeyStore();
    await keyStore.setKey(this.networkId, config.nearAccountId, nearAPI.utils.KeyPair.fromString(this.walletPrivateKey));
    this.provider = await connect({
      networkId: this.networkId,
      nodeUrl: this.rpcUrl,
      keyStore
    });
    this.wallet = await this.provider.account(config.nearAccountId);
    await logger(`NEAR Adapter khởi tạo với tài khoản: ${this.wallet.accountId}`);
  }

  async getTokenBalance(tokenAddress) {
    const balance = await this.wallet.viewFunction(tokenAddress, 'ft_balance_of', { account_id: this.wallet.accountId });
    return balance;
  }

  async watchMempool(callback) {
    this.provider.connection.provider.on('block', (block) => callback(block));
  }

  async buyToken(tokenAddress, amount) {
    const tx = await retry(() =>
      this.wallet.functionCall({
        contractId: 'v2.ref-finance.near', // Ref Finance
        methodName: 'ft_transfer_call',
        args: { receiver_id: tokenAddress, amount },
        gas: '30000000000000'
      })
    );
    await logger(`Mua token ${tokenAddress} với ${amount} NEAR`);
    return tx;
  }

  async sellToken(tokenAddress, amount) {
    const balance = await this.getTokenBalance(tokenAddress);
    if (balance < amount) throw new Error('Số dư token không đủ');
    const tx = await retry(() =>
      this.wallet.functionCall({
        contractId: tokenAddress,
        methodName: 'ft_transfer',
        args: { receiver_id: 'v2.ref-finance.near', amount },
        gas: '30000000000000'
      })
    );
    await logger(`Bán token ${tokenAddress} với ${amount} token`);
    return tx;
  }
}

export default NearAdapter;