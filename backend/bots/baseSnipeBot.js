import retry from '../utils/retry.js';
import logger from '../utils/logger.js';
import TradeLogic from '../logic_module/tradeLogic.js';
import UserService from '../services/userService.js';
import { ethers } from 'ethers';

class BaseSnipeBot {
  constructor({ walletPrivateKey, chains = { evm: 'https://eth.llamarpc.com' }, tslConfig = { dropPercent: 5, checkInterval: 60000 }, userId }) {
    this.walletPrivateKey = walletPrivateKey;
    this.chains = chains;
    this.tslConfig = tslConfig;
    this.userId = userId;
    this.providers = {};
    this.wallets = {};
    this.tradeLogics = {};
    this.activeChains = Object.keys(chains);
    this.userService = new UserService();
  }

  async initialize() {
    for (const [chain, rpcUrl] of Object.entries(this.chains)) {
      if (this.activeChains.includes(chain)) {
        this.providers[chain] = this.createProvider(chain, rpcUrl);
        this.wallets[chain] = this.createWallet(chain, this.providers[chain]);
        this.tradeLogics[chain] = new TradeLogic({ chain, rpcUrl, tslConfig: this.tslConfig, role: (await this.userService.checkRole(this.userId)).role });
        await this.tradeLogics[chain].initialize();
        await logger(`Khởi tạo ${chain} với ví: ${this.wallets[chain].address}`);
      }
    }
    await logger('BaseSnipeBot multi-chain đã khởi tạo');
  }

  createProvider(chain, rpcUrl) {
    if (chain === 'evm') return new ethers.JsonRpcProvider(rpcUrl);
    throw new Error(`Chain ${chain} chưa được hỗ trợ`);
  }

  createWallet(chain, provider) {
    if (chain === 'evm') return new ethers.Wallet(this.walletPrivateKey, provider);
    throw new Error(`Chain ${chain} chưa được hỗ trợ`);
  }

  async getBalance(chain) {
    const balance = await retry(() => this.providers[chain].getBalance(this.wallets[chain].address));
    return ethers.formatEther(balance);
  }

  async checkBalances() {
    const balances = {};
    for (const chain of this.activeChains) {
      balances[chain] = await this.getBalance(chain);
      await logger(`Số dư ví trên ${chain}: ${balances[chain]}`);
    }
    return balances;
  }

  // Manual Mode: Đánh giá token
  async evaluateToken(tokenInput) {
    const chain = this.activeChains[0];
    if (parseFloat(await this.getBalance(chain)) <= 0) throw new Error('Số dư không đủ');
    return await this.tradeLogics[chain].evaluateToken(tokenInput);
  }

  // Manual Mode: Trade với gas tùy chỉnh
  async manualTrade(tokenAddress, amount, action, customGas) {
    const chain = this.activeChains[0];
    if (parseFloat(await this.getBalance(chain)) <= 0) throw new Error('Số dư không đủ');
    return await this.tradeLogics[chain].manualTrade(tokenAddress, amount, action, customGas, {
      buyToken: this.buyToken.bind(this),
      sellToken: this.sellToken.bind(this)
    });
  }

  // Auto Mode: Smart Buy/Sell
  async autoTrade(tokenAddress, amount, action) {
    const chain = this.activeChains[0];
    const user = await this.userService.checkRole(this.userId);
    if (parseFloat(await this.getBalance(chain)) <= 0) throw new Error('Số dư không đủ');
    if (action === 'buy') {
      await this.userService.useTurn(this.userId);
      return await this.tradeLogics[chain].smartBuy(tokenAddress, amount, {
        buyToken: this.buyToken.bind(this),
        watchMempool: this.watchMempool.bind(this)
      });
    } else if (action === 'sell') {
      return await this.tradeLogics[chain].smartSell(tokenAddress, amount, {
        sellToken: this.sellToken.bind(this)
      });
    }
  }

  async buyToken(chain, tokenAddress, amount, gasPrice) {
    throw new Error('Phương thức buyToken phải được triển khai bởi adapter');
  }

  async sellToken(chain, tokenAddress, amount, gasPrice) {
    throw new Error('Phương thức sellToken phải được triển khai bởi adapter');
  }

  async watchMempool(callback, chain) {
    throw new Error('Phương thức watchMempool phải được triển khai bởi adapter');
  }

  setActiveChains(chains) {
    this.activeChains = chains.filter(chain => this.chains[chain]);
    logger(`Đã chọn các chain hoạt động: ${this.activeChains.join(', ')}`);
  }
}

export default BaseSnipeBot;