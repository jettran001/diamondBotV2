import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import { JsonRpcProvider, Ed25519Keypair } from '@mysten/sui.js';

class SuiAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, rpcUrl }) {
    super({ walletPrivateKey, rpcUrl });
  }

  async initialize() {
    this.provider = new JsonRpcProvider(this.rpcUrl);
    this.wallet = Ed25519Keypair.fromSecretKey(Buffer.from(this.walletPrivateKey, 'base64'));
    await logger(`SUI Adapter khởi tạo với ví: ${this.wallet.getPublicKey().toSuiAddress()}`);
  }

  async watchMempool(callback) {
    this.provider.subscribeToEvents({ onEvent: (event) => callback(event) });
  }

  async checkResources() {
    const resources = await this.provider.getOwnedObjects(this.wallet.getPublicKey().toSuiAddress());
    return resources.some(r => r.type === 'Coin<SUI>' && r.balance > 0);
  }

  async buyToken(tokenAddress, amount) {
    if (!(await this.checkResources())) throw new Error('Không đủ tài nguyên SUI');
    const tx = await retry(() =>
      this.provider.executeMoveCall({
        packageObjectId: 'CETUS_PACKAGE_ID',
        module: 'swap',
        function: 'swap',
        typeArguments: [tokenAddress],
        arguments: [amount],
        signer: this.wallet
      })
    );
    await logger(`Mua token ${tokenAddress} với ${amount} SUI`);
    return tx;
  }

  async sellToken(tokenAddress, amount) {
    if (!(await this.checkResources())) throw new Error('Không đủ tài nguyên SUI');
    const tx = await retry(() =>
      this.provider.executeMoveCall({
        packageObjectId: 'CETUS_PACKAGE_ID',
        module: 'swap',
        function: 'swap',
        typeArguments: [tokenAddress],
        arguments: [amount],
        signer: this.wallet
      })
    );
    await logger(`Bán token ${tokenAddress} với ${amount} token`);
    return tx;
  }
}

export default SuiAdapter;