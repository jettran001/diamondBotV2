import BaseSnipeBot from '../baseSnipeBot.js';
import retry from '../../utils/retry.js';
import logger from '../../utils/logger.js';
import { ethers } from 'ethers';
import { UniswapV2Router02ABI } from './abis/uniswapV2Router.js';

class EvmAdapter extends BaseSnipeBot {
  constructor({ walletPrivateKey, chains, tslConfig, userId }) {
    super({ walletPrivateKey, chains, tslConfig, userId });
  }

  async watchMempool(callback, chain) {
    if (chain !== 'evm') return;
    this.providers[chain].on('pending', async (txHash) => {
      const tx = await retry(() => this.providers[chain].getTransaction(txHash));
      if (tx) callback(tx);
    });
  }

  async buyToken(chain, tokenAddress, amount, gasPrice) {
    if (chain !== 'evm') throw new Error('Chain không hỗ trợ');
    const router = new ethers.Contract(this.getRouterAddress(), UniswapV2Router02ABI, this.wallets[chain]);
    const tx = await retry(() =>
      router.swapExactETHForTokens(
        0,
        [ethers.constants.AddressZero, tokenAddress],
        this.wallets[chain].address,
        Math.floor(Date.now() / 1000) + 60 * 10,
        { value: ethers.parseEther(amount), ...(gasPrice || {}) }
      )
    );
    await logger(`Mua token ${tokenAddress} với ${amount} ETH trên ${chain}`);
    return tx.wait();
  }

  async sellToken(chain, tokenAddress, amount, gasPrice) {
    if (chain !== 'evm') throw new Error('Chain không hỗ trợ');
    const router = new ethers.Contract(this.getRouterAddress(), UniswapV2Router02ABI, this.wallets[chain]);
    const tx = await retry(() =>
      router.swapExactTokensForETH(
        ethers.parseUnits(amount, 18),
        0,
        [tokenAddress, ethers.constants.AddressZero],
        this.wallets[chain].address,
        Math.floor(Date.now() / 1000) + 60 * 10,
        { ...(gasPrice || {}) }
      )
    );
    await logger(`Bán token ${tokenAddress} với ${amount} token trên ${chain}`);
    return tx.wait();
  }

  getRouterAddress() {
    return '0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D';
  }
}

export default EvmAdapter;