import logger from '../utils/logger.js';
import TokenStatus from './tokenStatus.js';
import RiskAnalyzer from './riskAnalyzer.js';
import optimizeGas from './gasOptimizer.js';
import NotificationService from '../services/notificationService.js';
import { ethers } from 'ethers';

class TradeLogic {
  constructor({ chain, rpcUrl, tslConfig = { dropPercent: 5, checkInterval: 60000 }, role = 'free' }) {
    this.chain = chain;
    this.rpcUrl = rpcUrl;
    this.tslConfig = tslConfig;
    this.tokenStatus = new TokenStatus(chain, rpcUrl);
    this.riskAnalyzer = new RiskAnalyzer(this.createProvider(chain, rpcUrl));
    this.notificationService = new NotificationService({
      telegramToken: process.env.TELEGRAM_TOKEN,
      discordWebhook: process.env.DISCORD_WEBHOOK
    });
    this.role = role;
    this.tokenCache = new Map();
  }

  async initialize() {
    await this.notificationService.initialize();
  }

  createProvider(chain, rpcUrl) {
    if (chain === 'evm') return new ethers.JsonRpcProvider(rpcUrl);
    throw new Error(`Chain ${chain} chưa được hỗ trợ`);
  }

  // Manual Mode: Đánh giá token
  async evaluateToken(tokenInput) {
    const tokenAddress = await this.resolveTokenInput(tokenInput);
    const tokenInfo = await this.tokenStatus.getTokenInfo(tokenAddress);
    const risks = await this.riskAnalyzer.analyzeLiquidity(tokenAddress, this.getRouterAddress());
    this.tokenCache.set(`${this.chain}:${tokenAddress}`, risks);

    const log = {
      tokenInfo,
      risks,
      potential: risks.level === '🟢' ? 'Cao' : risks.level === '🟡' ? 'Trung bình' : 'Thấp'
    };
    await logger(`Đánh giá ${tokenAddress}: ${JSON.stringify(log)}`);
    return log;
  }

  async resolveTokenInput(input) {
    if (ethers.utils.isAddress(input)) return input;
    // TODO: Resolve qua API (CoinGecko) nếu là name/symbol
    throw new Error('Chưa hỗ trợ tìm token qua name/symbol');
  }

  // Manual Mode: Trade với gas tùy chỉnh
  async manualTrade(tokenAddress, amount, action, customGas, { buyToken, sellToken }) {
    const risks = this.tokenCache.get(`${this.chain}:${tokenAddress}`) || await this.riskAnalyzer.analyzeLiquidity(tokenAddress, this.getRouterAddress());
    const gasPrice = customGas || ((this.role === 'premium' || this.role === 'ultimate') ? await optimizeGas(this.createProvider(this.chain, this.rpcUrl)) : null);

    if (action === 'buy') {
      return this.executeSmartBuy(tokenAddress, amount, risks, { gasPrice }, { buyToken });
    } else if (action === 'sell') {
      return this.executeSmartSell(tokenAddress, amount, risks, { gasPrice }, { sellToken });
    }
  }

  // Auto Mode: Smart Buy
  async smartBuy(tokenAddress, amount, { buyToken, watchMempool }) {
    const risks = this.tokenCache.get(`${this.chain}:${tokenAddress}`) || await this.riskAnalyzer.analyzeLiquidity(tokenAddress, this.getRouterAddress());
    this.tokenCache.set(`${this.chain}:${tokenAddress}`, risks);
    const mempoolData = this.role === 'ultimate' ? await this.analyzeMempool(tokenAddress) : { potentialScore: 0 };
    return this.executeSmartBuy(tokenAddress, amount, risks, mempoolData, { buyToken, watchMempool });
  }

  async analyzeMempool(tokenAddress) {
    const provider = this.createProvider(this.chain, this.rpcUrl);
    const pendingTxs = [];
    await new Promise((resolve) => {
      provider.on('pending', async (txHash) => {
        const tx = await retry(() => provider.getTransaction(txHash));
        if (tx && tx.to === tokenAddress) pendingTxs.push(tx);
        if (pendingTxs.length >= 10) resolve();
      });
      setTimeout(resolve, 5000);
    });
    const volume = pendingTxs.reduce((sum, tx) => sum.add(tx.value || ethers.BigNumber.from(0)), ethers.BigNumber.from(0));
    const avgGas = pendingTxs.reduce((sum, tx) => sum.add(tx.gasPrice || ethers.BigNumber.from(0)), ethers.BigNumber.from(0)).div(pendingTxs.length || 1);
    const score = Math.min(100, pendingTxs.length * 10 + ethers.formatEther(volume) * 10 + ethers.formatUnits(avgGas, 'gwei') / 100);
    return { txCount: pendingTxs.length, volume: ethers.formatEther(volume), avgGas: ethers.formatUnits(avgGas, 'gwei'), potentialScore: score };
  }

  async executeSmartBuy(tokenAddress, amount, risks, mempoolData, { buyToken, watchMempool }) {
    const gasPrice = (this.role === 'premium' || this.role === 'ultimate') ? await optimizeGas(this.createProvider(this.chain, this.rpcUrl)) : null;

    switch (risks.level) {
      case '🔴':
        await logger(`Bỏ qua mua ${tokenAddress}: Rủi ro cao - ${risks.details.join(', ')}`);
        await this.notificationService.sendNotification(`🔴 Bỏ qua ${tokenAddress}: ${risks.details.join(', ')}`);
        return { action: 'skip', reason: risks.details };
      case '🟡':
        if (this.role === 'free') return { action: 'skip', reason: 'Free user không được phép mua token 🟡' };
        await logger(`Mua thận trọng ${tokenAddress}: Score ${risks.score}`);
        const buyTxYellow = await buyToken(this.chain, tokenAddress, amount, gasPrice);
        setTimeout(async () => {
          const priceHistory = await this.tokenStatus.api.getPriceHistory(tokenAddress);
          const priceChange = (priceHistory[priceHistory.length - 1].price / priceHistory[0].price - 1) * 100;
          if (priceChange > 5) {
            await this.sellToken(this.chain, tokenAddress, amount);
            await this.notificationService.sendNotification(`🟡 Bán ${tokenAddress}: Tăng ${priceChange}%`);
          }
        }, 5 * 60 * 1000);
        return { action: 'buy', tx: buyTxYellow };
      case '🟢':
        if (this.role === 'free' || this.role === 'premium') {
          await logger(`Mua cơ bản ${tokenAddress}: Score ${risks.score}`);
          const buyTxGreen = await buyToken(this.chain, tokenAddress, amount, gasPrice);
          return { action: 'buy', tx: buyTxGreen };
        }
        if (this.role === 'ultimate') {
          await logger(`Front-run mua ${tokenAddress}: Score ${risks.score}, Mempool Score ${mempoolData.potentialScore}`);
          await this.notificationService.sendNotification(`🟢 Bắt đầu front-run ${tokenAddress}`);
          watchMempool(async (tx) => {
            if (tx.value?.gt(ethers.parseEther('1'))) {
              const frontTx = await buyToken(this.chain, tokenAddress, amount, gasPrice);
              let lastPrice = await this.tokenStatus.api.getTokenPrice(tokenAddress);
              const tsl = setInterval(async () => {
                const currentPrice = await this.tokenStatus.api.getTokenPrice(tokenAddress);
                if (currentPrice < lastPrice * (1 - this.tslConfig.dropPercent / 100)) {
                  await this.sellToken(this.chain, tokenAddress, amount);
                  await this.notificationService.sendNotification(`🟢 TSL bán ${tokenAddress}: Giảm ${this.tslConfig.dropPercent}%`);
                  clearInterval(tsl);
                } else if (mempoolData.volume > 100 && currentPrice > lastPrice * 1.1) {
                  await buyToken(this.chain, tokenAddress, amount / 2, gasPrice);
                  await logger(`Mua thêm ${tokenAddress}: Pump mạnh`);
                }
                lastPrice = Math.max(lastPrice, currentPrice);
              }, this.tslConfig.checkInterval);
              return { action: 'front-run', tx: frontTx };
            }
          }, this.chain);
          return { action: 'front-run-started' };
        }
    }
  }

  // Auto Mode: Smart Sell
  async smartSell(tokenAddress, amount, { sellToken }) {
    const risks = this.tokenCache.get(`${this.chain}:${tokenAddress}`) || await this.riskAnalyzer.analyzeLiquidity(tokenAddress, this.getRouterAddress());
    this.tokenCache.set(`${this.chain}:${tokenAddress}`, risks);
    return this.executeSmartSell(tokenAddress, amount, risks, { sellToken });
  }

  async executeSmartSell(tokenAddress, amount, risks, { sellToken }) {
    const gasPrice = (this.role === 'premium' || this.role === 'ultimate') ? await optimizeGas(this.createProvider(this.chain, this.rpcUrl)) : null;
    switch (risks.level) {
      case '🔴':
        await logger(`Bỏ qua bán ${tokenAddress}: Rủi ro cao - ${risks.details.join(', ')}`);
        await this.notificationService.sendNotification(`🔴 Bỏ qua ${tokenAddress}: ${risks.details.join(', ')}`);
        return { action: 'skip', reason: risks.details };
      case '🟡':
      case '🟢':
        await logger(`Bán ${tokenAddress}: Score ${risks.score}`);
        const sellTx = await sellToken(this.chain, tokenAddress, amount, gasPrice);
        await this.notificationService.sendNotification(`✅ Bán ${tokenAddress} thành công`);
        return { action: 'sell', tx: sellTx };
    }
  }

  getRouterAddress() {
    return '0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D'; // Uniswap V2 Router
  }
}

export default TradeLogic;