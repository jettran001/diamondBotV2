import dotenv from 'dotenv';

dotenv.config();

export default {
  reserveWallet: '0xYourReserveWalletAddress',
  rpcUrls: {
    evm: 'https://eth.llamarpc.com'
  },
  nearAccountId: process.env.NEAR_ACCOUNT_ID,
  rpcUrls: {
    evm: process.env.RPC_EVM,
    ton: process.env.RPC_TON,
    near: process.env.RPC_NEAR,
    solana: process.env.RPC_SOLANA,
    sui: process.env.RPC_SUI
  },
  chainIds: {
    evm: 1,    // Ethereum Mainnet
    bsc: 56,   // Binance Smart Chain
    polygon: 137, // Polygon
    ton: 'ton-mainnet',
    near: 'near-testnet',
    solana: 'solana-mainnet',
    sui: 'sui-mainnet'
  },
  apiKeys: {
    coingecko: process.env.API_COINGECKO,
    binance: process.env.API_BINANCE,
    terminal: process.env.API_TERMINAL,
    telegram: process.env.TELEGRAM_TOKEN,
    discord: process.env.DISCORD_WEBHOOK
  },
  mongoUrl: process.env.MONGO_URL,
  ipfsUrl: process.env.IPFS_URL,
  redisUrl: process.env.REDIS_URL
};