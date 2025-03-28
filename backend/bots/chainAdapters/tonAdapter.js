import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import { TonClient, JettonMaster } from '@ton/ton';

class TonAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, rpcUrl }) {
    super({ walletPrivateKey, rpcUrl });
  }

  async initialize() {
    this.provider = new TonClient({ endpoint: this.rpcUrl });
    this.wallet = await this.provider.openWalletFromSecretKey(this.walletPrivateKey);
    await logger(`TON Adapter khởi tạo với ví: ${this.wallet.address}`);
  }

  async watchMempool(callback) {
    this.provider.on('transaction', (tx) => callback(tx));
  }

  async buyToken(tokenAddress, amount) {
    const jettonMaster = new JettonMaster(this.provider, tokenAddress);
    const tx = await retry(() =>
      this.wallet.send({
        to: jettonMaster.address,
        value: amount, // nanoTON
        data: jettonMaster.createTransferBody({
          jettonAmount: amount,
          toAddress: this.wallet.address
        })
      })
    );
    const receipt = await this.provider.waitForTransaction(tx.txId);
    await logger(`Mua JETTON ${tokenAddress} với ${amount} nanoTON`);
    return receipt;
  }

  async sellToken(tokenAddress, amount) {
    const jettonMaster = new JettonMaster(this.provider, tokenAddress);
    const tx = await retry(() =>
      this.wallet.send({
        to: jettonMaster.address,
        value: amount,
        data: jettonMaster.createTransferBody({
          jettonAmount: amount,
          toAddress: 'STON_FI_ADDRESS' // Địa chỉ STON.fi DEX
        })
      })
    );
    const receipt = await this.provider.waitForTransaction(tx.txId);
    await logger(`Bán JETTON ${tokenAddress} với ${amount} nanoTON`);
    return receipt;
  }
}

export default TonAdapter;