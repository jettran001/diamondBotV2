import logger from '../utils/logger.js';
import { ethers } from 'ethers';
import { TonClient, JettonMaster, Address } from '@ton/ton';
import { Connection, PublicKey } from '@solana/web3.js';
import api from '../utils/api.js';
import config from '../config/config.js';

class TokenStatus {
  constructor(chain = 'evm', rpcUrl = config.rpcUrls.evm) {
    this.chain = chain;
    if (chain === 'evm') this.provider = new ethers.JsonRpcProvider(rpcUrl);
    else if (chain === 'ton') this.provider = new TonClient({ endpoint: rpcUrl });
    else if (chain === 'solana') this.provider = new Connection(rpcUrl);
  }

  // Lấy thông tin token
  async getTokenInfo(tokenAddress) {
    if (this.chain === 'evm') {
      const tokenAbi = [
        'function name() view returns (string)',
        'function symbol() view returns (string)',
        'function totalSupply() view returns (uint256)',
        'function decimals() view returns (uint8)'
      ];
      const token = new ethers.Contract(tokenAddress, tokenAbi, this.provider);
      try {
        const [name, symbol, totalSupply, decimals] = await Promise.all([
          token.name(),
          token.symbol(),
          token.totalSupply(),
          token.decimals()
        ]);
        const supplyFormatted = ethers.formatUnits(totalSupply, decimals);
        await logger(`Token ${tokenAddress}: ${name} (${symbol}), Total Supply: ${supplyFormatted}`);
        return { name, symbol, totalSupply: supplyFormatted, decimals };
      } catch (error) {
        await logger(`Lỗi kiểm tra token ${tokenAddress}: ${error.message}`, 'error');
        return null;
      }
    } else if (this.chain === 'ton') {
      const jetton = new JettonMaster(this.provider, Address.parse(tokenAddress));
      const data = await jetton.getJettonData();
      await logger(`Token TON ${tokenAddress}: ${data.name} (${data.symbol}), Total Supply: ${data.totalSupply}`);
      return { name: data.name, symbol: data.symbol, totalSupply: data.totalSupply.toString() };
    } else if (this.chain === 'solana') {
      const token = new PublicKey(tokenAddress);
      const info = await this.provider.getTokenSupply(token);
      await logger(`Token Solana ${tokenAddress}: Total Supply: ${info.value.uiAmountString}`);
      return { name: 'Unknown', symbol: 'Unknown', totalSupply: info.value.uiAmountString };
    }
  }

  // Kiểm tra honeypot và phân tích rủi ro
  async analyzeTokenRisk(tokenAddress) {
    const risks = { level: '🟢', details: [], score: 100 };

    // 1. Kiểm tra API bên ngoài (DexTools, RugCheck, Honeypot.is)
    await this.checkExternalAPIs(tokenAddress, risks);

    // 2. Kiểm tra hợp đồng (EVM-specific)
    if (this.chain === 'evm') {
      await this.checkEvmContract(tokenAddress, risks);
      await this.checkTransactionHistory(tokenAddress, risks);
    } else if (this.chain === 'ton') {
      await this.checkTonContract(tokenAddress, risks);
    } else if (this.chain === 'solana') {
      await this.checkSolanaContract(tokenAddress, risks);
    }

    // 3. Đánh giá mức độ rủi ro
    if (risks.score < 60 || risks.details.some(d => d.includes('🔴'))) {
      risks.level = '🔴';
    } else if (risks.score < 80) {
      risks.level = '🟡';
    }

    await logger(`Phân tích rủi ro ${tokenAddress}: ${risks.level} - Score: ${risks.score}`);
    return risks;
  }

  async checkExternalAPIs(tokenAddress, risks) {
    const apis = [
      { name: 'dextools', path: `/token/${this.chain}/${tokenAddress}/security` },
      { name: 'rugcheck', path: `/check/${tokenAddress}` },
      { name: 'honeypot', path: `/is-honeypot?address=${tokenAddress}&chain=${this.chain}` }
    ];

    for (const apiCheck of apis) {
      try {
        const data = await api.get(apiCheck.name, apiCheck.path);
        if (apiCheck.name === 'dextools') {
          if (data.isHoneypot || data.lockedLiquidity < 0.5) {
            risks.details.push('🔴 Honeypot hoặc thanh khoản khóa < 50% (DexTools)');
            risks.score -= 40;
          }
          risks.score = Math.max(0, Math.min(100, data.auditScore || risks.score));
        } else if (apiCheck.name === 'rugcheck' && data.riskLevel === 'high') {
          risks.details.push('🔴 Rủi ro cao (RugCheck)');
          risks.score -= 50;
        } else if (apiCheck.name === 'honeypot' && data.isHoneypot) {
          risks.details.push('🔴 Xác định honeypot (Honeypot.is)');
          risks.score -= 50;
        }
      } catch (error) {
        await logger(`Lỗi API ${apiCheck.name} cho ${tokenAddress}: ${error.message}`, 'warn');
      }
    }
  }

  async checkEvmContract(tokenAddress, risks) {
    const token = new ethers.Contract(tokenAddress, [
      'function balanceOf(address) view returns (uint256)',
      'function owner() view returns (address)',
      'function totalSupply() view returns (uint256)',
      'function decimals() view returns (uint8)',
      'function pause()',
      'function blacklistAddress(address)'
    ], this.provider);

    // Honeypot
    const deadBalance = await token.balanceOf('0x000000000000000000000000000000000000dEaD');
    if (deadBalance.gt(0)) {
      risks.details.push('🔴 Honeypot: Dead address có token');
      risks.score -= 50;
    }

    // Ownership
    const owner = await token.owner().catch(() => ethers.constants.AddressZero);
    if (owner === ethers.constants.AddressZero) {
      risks.details.push('🟡 Ownership từ bỏ hoặc ẩn');
      risks.score -= 20;
    } else {
      const ownerBalance = await token.balanceOf(owner);
      const totalSupply = await token.totalSupply();
      const ownershipPercent = ownerBalance.mul(100).div(totalSupply).toNumber();
      if (ownershipPercent > 50) {
        risks.details.push('🔴 Ownership > 50%: Tập trung cao');
        risks.score -= 30;
      }
    }

    // Pausable/Blacklist
    try {
      await token.pause();
      risks.details.push('🔴 Pausable: Có hàm pause()');
      risks.score -= 40;
    } catch {}
    try {
      await token.blacklistAddress(ethers.constants.AddressZero);
      risks.details.push('🔴 Blacklist: Có hàm chặn ví');
      risks.score -= 40;
    } catch {}

    // Liquidity
    const pairAddress = ethers.utils.getCreate2Address('0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f', tokenAddress, ethers.constants.AddressZero);
    const pair = new ethers.Contract(pairAddress, ['function getReserves() view returns (uint112, uint112, uint32)'], this.provider);
    const [reserve0, reserve1] = await pair.getReserves().catch(() => [0, 0]);
    const liquidityUsd = ethers.formatEther(reserve0.add(reserve1)) * 2000; // Giả lập giá ETH
    if (liquidityUsd < 1000) {
      risks.details.push('🔴 Thanh khoản < $1,000');
      risks.score -= 30;
    } else if (liquidityUsd > 10000) {
      risks.score += 20;
    }
  }

  async checkTransactionHistory(tokenAddress, risks) {
    const txs = await this.provider.getHistory(tokenAddress, null, null, 50); // 50 tx gần nhất
    const totalVolume = txs.reduce((sum, tx) => sum.add(tx.value || 0), ethers.BigNumber.from(0));
    const avgVolume = ethers.formatEther(totalVolume.div(txs.length));
    if (avgVolume > 100 && txs.length < 10) {
      risks.details.push('🔴 Volume giả: Giao dịch ít nhưng giá trị lớn');
      risks.score -= 30;
    }
  }

  async checkTonContract(tokenAddress, risks) {
    const jetton = new JettonMaster(this.provider, Address.parse(tokenAddress));
    const data = await jetton.getJettonData();
    const adminAddress = data.adminAddress?.toString() || '';
    const burnBalance = await jetton.getWalletData(Address.parse('EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c')).then(wallet => wallet.balance);
    if (burnBalance.eq(0) && adminAddress === '') {
      risks.details.push('🔴 TON Honeypot: Không burn, không admin');
      risks.score -= 40;
    }
  }

  async checkSolanaContract(tokenAddress, risks) {
    const token = new PublicKey(tokenAddress);
    const mintInfo = await this.provider.getAccountInfo(token);
    const freezeAuthority = mintInfo.data.parsed.info.freezeAuthority;
    const mintAuthority = mintInfo.data.parsed.info.mintAuthority;
    if (!freezeAuthority || !mintAuthority) {
      risks.details.push('🔴 Solana Honeypot: Thiếu authority');
      risks.score -= 40;
    }
  }

  // Logic smart buy/sell
  async smartTrade(tokenAddress, amount, bot) {
    const risks = await this.analyzeTokenRisk(tokenAddress);
    switch (risks.level) {
      case '🔴':
        await logger(`Bỏ qua ${tokenAddress}: Rủi ro cao - ${risks.details.join(', ')}`);
        return { action: 'skip', reason: risks.details };
      case '🟡':
        await logger(`Mua thận trọng ${tokenAddress}: ${risks.score}`);
        const buyTx = await bot.buyToken(tokenAddress, amount);
        setTimeout(async () => {
          const priceHistory = await api.getPriceHistory(tokenAddress);
          const priceChange = (priceHistory[priceHistory.length - 1].price / priceHistory[0].price - 1) * 100;
          if (priceChange > 5) await bot.sellToken(tokenAddress, amount);
        }, 5 * 60 * 1000); // 5 phút
        return { action: 'buy', tx: buyTx };
      case '🟢':
        await logger(`Front-run ${tokenAddress}: ${risks.score}`);
        let trades = 0;
        bot.watchMempool(async (tx) => {
          if (trades < 3 && tx.value?.gt(ethers.parseEther('1'))) {
            const frontTx = await bot.buyToken(tokenAddress, amount);
            trades++;
            // Trailing Stop Loss (TSL)
            const tsl = setInterval(async () => {
              const price = await api.getTokenPrice(tokenAddress);
              if (price < price * 0.95) {
                await bot.sellToken(tokenAddress, amount);
                clearInterval(tsl);
              }
            }, 60000); // Kiểm tra mỗi phút
            return { action: 'front-run', tx: frontTx };
          }
        });
        return { action: 'front-run-started' };
    }
  }
}

export default TokenStatus;