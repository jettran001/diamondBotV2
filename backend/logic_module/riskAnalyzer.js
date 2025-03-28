import logger from '../utils/logger.js';
import { ethers } from 'ethers';
import api from '../utils/api.js';

class RiskAnalyzer {
  constructor(provider) {
    this.provider = provider;
  }

  async analyzeLiquidity(tokenAddress, routerAddress) {
    const pairAbi = ['function getReserves() external view returns (uint112, uint112, uint32)'];
    const router = new ethers.Contract(routerAddress, ['function factory() view returns (address)'], this.provider);
    const factoryAddress = await router.factory();
    const pairAddress = ethers.utils.getCreate2Address(factoryAddress, tokenAddress, ethers.constants.AddressZero);
    const pair = new ethers.Contract(pairAddress, pairAbi, this.provider);

    try {
      const [reserve0, reserve1] = await pair.getReserves();
      const locked = await pair.lockedLiquidity(); // Giả lập kiểm tra lock
      const liquidityEth = ethers.formatEther(reserve0.add(reserve1));
      const riskScore = liquidityEth < 10 || !locked ? 'Cao' : liquidityEth < 50 ? 'Trung bình' : 'Thấp';
      await logger(`Thanh khoản ${tokenAddress}: ${liquidityEth} ETH - Rủi ro: ${riskScore}`);
      return { liquidityEth, riskScore, locked };
    } catch (error) {
      return { liquidityEth: 0, riskScore: 'Rất cao', locked: false };
    }
  }

  async analyzeVolatility(tokenId) {
    const history = await api.getPriceHistory(tokenId);
    const prices = history.map(p => p.price);
    const avgPrice = prices.reduce((sum, p) => sum + p, 0) / prices.length;
    const variance = prices.reduce((sum, p) => sum + Math.pow(p - avgPrice, 2), 0) / prices.length;
    const volatility = Math.sqrt(variance);
    const riskScore = volatility > 0.5 ? 'Cao' : volatility > 0.2 ? 'Trung bình' : 'Thấp';
    await logger(`Biến động ${tokenId}: ${volatility} - Rủi ro: ${riskScore}`);
    return { volatility, riskScore };
  }

  async checkRugPull(tokenAddress) {
    const txs = await this.provider.getHistory(tokenAddress, null, null, 100); // Lấy 100 tx gần nhất
    const burnTxs = txs.filter(tx => tx.to === ethers.constants.AddressZero);
    const rugRisk = burnTxs.length > 5 ? 'Cao' : burnTxs.length > 1 ? 'Trung bình' : 'Thấp';
    await logger(`Kiểm tra rug-pull ${tokenAddress}: ${burnTxs.length} tx burn - Rủi ro: ${rugRisk}`);
    return { burnCount: burnTxs.length, rugRisk };
  }
}

export default RiskAnalyzer;