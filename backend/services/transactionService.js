import BaseSnipeBot from '../bots/baseSnipeBot.js';
import logger from '../utils/logger.js';

class TransactionService {
  constructor(botConfig) {
    this.bot = new BaseSnipeBot(botConfig);
  }

  async initialize() {
    await this.bot.initialize();
    await logger('TransactionService đã khởi tạo');
  }

  async buyToken(tokenAddress, amount) {
    const chain = this.bot.activeChains[0];
    return await this.bot.autoTrade(tokenAddress, amount, 'buy');
  }

  async sellToken(tokenAddress, amount) {
    const chain = this.bot.activeChains[0];
    return await this.bot.autoTrade(tokenAddress, amount, 'sell');
  }
}

export default TransactionService;