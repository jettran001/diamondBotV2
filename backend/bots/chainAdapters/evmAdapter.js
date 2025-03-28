import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import { ethers } from 'ethers';
import UniswapV2Router02ABI from './abis/uniswapV2Router.js'; // Đổi tên để rõ ràng

class EvmAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, chains, tslConfig, role }) {
    super({ walletPrivateKey, chains, tslConfig, role });
    this.supportedChains = {
      1: { name: 'ethereum', weth: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2' }, // Ethereum Mainnet
      56: { name: 'bsc', weth: '0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c' }     // Binance Smart Chain
    };
  }

  async watchMempool(callback, chainId) {
    const chain = this.supportedChains[chainId];
    if (!chain) throw new Error(`Chain ID ${chainId} không hỗ trợ`);
    this.providers[chain.name].on('pending', async (txHash) => {
      const tx = await retry(() => this.providers[chain.name].getTransaction(txHash));
      if (tx) callback(tx);
    });
  }

  async buyToken(chainId, tokenAddress, amount, gasPrice) {
    const chain = this.supportedChains[chainId];
    if (!chain) throw new Error(`Chain ID ${chainId} không hỗ trợ`);
    const router = new ethers.Contract(
      this.getRouterAddress(chainId),
      UniswapV2Router02ABI,
      this.wallets[chain.name]
    );
    const tx = await retry(() =>
      router.swapExactETHForTokens(
        0, // amountOutMin
        [chain.weth, tokenAddress], // Path: WETH/WBNB -> Token
        this.wallets[chain.name].address,
        Math.floor(Date.now() / 1000) + 60 * 10,
        { value: ethers.parseEther(amount), ...(gasPrice || {}) }
      )
    );
    await logger(`Mua token ${tokenAddress} với ${amount} ETH trên ${chain.name}`);
    return tx.wait();
  }

  async sellToken(chainId, tokenAddress, amount, gasPrice) {
    const chain = this.supportedChains[chainId];
    if (!chain) throw new Error(`Chain ID ${chainId} không hỗ trợ`);
    const router = new ethers.Contract(
      this.getRouterAddress(chainId),
      UniswapV2Router02ABI,
      this.wallets[chain.name]
    );
    const tx = await retry(() =>
      router.swapExactTokensForETH(
        ethers.parseUnits(amount, 18),
        0, // amountOutMin
        [tokenAddress, chain.weth], // Path: Token -> WETH/WBNB
        this.wallets[chain.name].address,
        Math.floor(Date.now() / 1000) + 60 * 10,
        { ...(gasPrice || {}) }
      )
    );
    await logger(`Bán token ${tokenAddress} với ${amount} token trên ${chain.name}`);
    return tx.wait();
  }

  getRouterAddress(chainId) {
    const routers = {
      1: '0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D', // Uniswap V2 (Ethereum)
      56: '0x10ED43C718714eb63d5aA57B78B54704E256024E' // PancakeSwap V2 (BSC)
    };
    return routers[chainId] || routers[56]; // Mặc định BSC nếu chainId không xác định
  }
}

export default EvmAdapter;