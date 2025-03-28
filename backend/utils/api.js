import axios from 'axios';
import logger from './logger.js';

class ApiClient {
  constructor() {
    this.clients = {
      coingecko: axios.create({ baseURL: 'https://api.coingecko.com/api/v3' }),
      binance: axios.create({ baseURL: 'https://api.binance.com/api/v3' }),
      terminal: axios.create({ baseURL: 'https://terminal-api.example.com' }),
      dexscreener: axios.create({ baseURL: 'https://api.dexscreener.com' }),
      dextools: axios.create({ baseURL: 'https://api.dextools.io' }),
      rugcheck: axios.create({ baseURL: 'https://api.rugcheck.xyz' }),
      honeypot: axios.create({ baseURL: 'https://honeypot.is/api' }),
      geckoterminal: axios.create({ baseURL: 'https://api.geckoterminal.com/api/v2' }) // Thêm GeckoTerminal
    };
  }

  async get(source, endpoint, params = {}) {
    const client = this.clients[source];
    try {
      const response = await client.get(endpoint, { params, headers: { 'X-API-KEY': process.env[`API_${source.toUpperCase()}`] } });
      await logger(`Gọi API ${source} ${endpoint} thành công`);
      return response.data;
    } catch (error) {
      await logger(`Lỗi API ${source} ${endpoint}: ${error.message}`, 'error');
      throw error;
    }
  }

  async getTokenPrice(tokenId) {
    const sources = ['coingecko', 'binance', 'dexscreener'];
    for (const source of sources) {
      try {
        if (source === 'coingecko') return (await this.get('coingecko', '/simple/price', { ids: tokenId, vs_currencies: 'usd' }))[tokenId]?.usd;
        if (source === 'binance') return (await this.get('binance', '/ticker/price', { symbol: `${tokenId}USDT` })).price;
        if (source === 'dexscreener') return (await this.get('dexscreener', `/tokens/${tokenId}`)).priceUsd;
      } catch (error) {
        continue;
      }
    }
    return 0;
  }

  async getPriceHistory(tokenId) {
    return await this.get('coingecko', `/coins/${tokenId}/market_chart`, { vs_currency: 'usd', days: 1 });
  }
}

export default new ApiClient();