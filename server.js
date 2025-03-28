import express from 'express';
import cors from 'cors';
import { createClient } from 'redis';
import WebSocket from 'ws';
import BotController from './backend/controllers/botController.js';
import UserController from './backend/controllers/userController.js';
import NotificationService from './backend/services/notificationService.js';
import logger from './backend/utils/logger.js';
import dotenv from 'dotenv';
import config from './backend/config/config.js';
import { Worker } from 'worker_threads';

dotenv.config();
const app = express();
app.use(express.json());
app.use(cors({ origin: 'https://your-frontend-url.com' }));

// Kết nối tới WebSocket server
const wsClient = new WebSocket(`ws://${process.env.WS_HOST}:${process.env.WS_PORT}`);
wsClient.on('open', () => logger('Connected to WebSocket server', 'info'));
wsClient.on('error', (err) => logger(`WebSocket client error: ${err.message}`, 'error'));

const broadcast = (message) => {
  if (wsClient.readyState === WebSocket.OPEN) {
    wsClient.send(JSON.stringify({ type: 'broadcast', message }));
  }
};

// Khởi tạo Redis client
const redis = createClient({
  url: 'redis://redis:6379', // Sửa thành redis thay vì localhost
  retry_strategy: () => ({
    attempts: parseInt(process.env.REDIS_RETRY_ATTEMPTS) || 10,
    delay: parseInt(process.env.REDIS_RETRY_DELAY) || 5000
  })
});

// Khởi tạo các service
const botController = new BotController({
  walletPrivateKey: process.env.WALLET_PRIVATE_KEY,
  chains: { evm: process.env.RPC_EVM || 'https://eth.llamarpc.com' },
  tslConfig: { dropPercent: 5, checkInterval: 60000 },
  userId: 'user123'
});

const userController = new UserController();
const notificationService = new NotificationService({
  telegramToken: config.apiKeys.telegram,
  discordWebhook: config.apiKeys.discord,
  broadcast
});

// Health endpoint
app.get('/health', (req, res) => res.status(200).send('OK'));

process.on('uncaughtException', async (err) => {
  await logger(`Uncaught Exception: ${err.stack}`, 'error');
  process.exit(1);
});

async function startServer() {
  try {
    await redis.connect();
    await logger(`Redis connected on ${redis.options.url}`, 'info');

    await botController.start();
    await userController.initialize();
    await notificationService.initialize();
    await logger('Server đã khởi động', 'info');

    app.listen(3000, () => logger('Server HTTP chạy trên port 3000'));
  } catch (error) {
    await logger(`Lỗi khởi động server: ${error.stack || error.message}`, 'error');
    process.exit(1);
  }
}

process.on('SIGTERM', async () => {
  wsClient.close();
  await redis.quit();
  await botController.stop();
  await notificationService.close();
  await logger('Server đã dừng', 'info');
  process.exit(0);
});

async function startWorkers() {
  const numWorkers = 2; // Dùng 2 workers
  for (let i = 0; i < numWorkers; i++) {
    new Worker('./backend/transactionWorker.js');
  }
}

startServer().then(startWorkers);