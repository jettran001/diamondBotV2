import { ethers } from 'ethers';
import logger from '../utils/logger.js';

async function optimizeGas(provider, historyWindow = 10) {
  const feeData = await provider.getFeeData();
  const gasPrice = feeData.gasPrice;
  const blockHistory = await Promise.all(
    Array.from({ length: historyWindow }, (_, i) => provider.getBlock(-i - 1))
  );
  const avgGasPrice = blockHistory.reduce((sum, block) => sum.add(block.baseFeePerGas || gasPrice), ethers.BigNumber.from(0))
    .div(historyWindow)
    .mul(110)
    .div(100); // Dự đoán dựa trên lịch sử + 10%

  const optimized = {
    gasPrice: avgGasPrice,
    maxFeePerGas: feeData.maxFeePerGas ? avgGasPrice.mul(120).div(100) : avgGasPrice, // Linh hoạt EIP-1559
    maxPriorityFeePerGas: feeData.maxPriorityFeePerGas || ethers.parseUnits('2', 'gwei')
  };
  await logger(`Gas tối ưu: ${ethers.formatUnits(optimized.maxFeePerGas, 'gwei')} gwei`);
  return optimized;
}

export default optimizeGas;