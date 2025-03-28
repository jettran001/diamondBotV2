import { Worker } from 'worker_threads';
import express from 'express';
import TransactionService from '../services/transactionService.js';
import NotificationService from '../services/notificationService.js';
import logger from '../utils/logger.js';
import { createClient } from 'redis';
import { isPortTaken } from '../utils/helpers.js';

class BotController {
  constructor(botConfig) {
    this.transactionService = new TransactionService(botConfig);
    this.notificationService = new NotificationService({});
    this.app = express();
    this.state = 'stopped';
    this.workers = [];
  }

  async start() {
    // Kiểm tra Redis
    const redisClient = createClient({ url: process.env.REDIS_URL || 'redis://localhost:6379' });
    try {
      await redisClient.connect();
      await logger('Redis đã kết nối thành công');
    } catch (error) {
      await logger(`Lỗi kết nối Redis: ${error.message}`, 'error');
      process.exit(1);
    } finally {
      await redisClient.quit();
    }

    // Kiểm tra cổng WebSocket (9000 là mặc định từ NotificationService)
    const wsPort = 9000;
    if (await isPortTaken(wsPort)) {
      await logger(`Cổng ${wsPort} đã bị chiếm, kiểm tra và thử lại`, 'error');
      process.exit(1);
    }

    // Tiếp tục khởi động nếu kiểm tra thành công
    this.state = 'running';
    await this.transactionService.initialize();
    await this.notificationService.initialize();
    this.setupApi();
    this.startWorker();
    await this.notificationService.sendNotification('Bot đã khởi động');
    await logger('BotController đã khởi động');
  }

  async stop() {
    this.state = 'stopped';
    this.workers.forEach(worker => worker.terminate());
    await this.notificationService.sendNotification('Bot đã dừng');
    await this.notificationService.close();
    await logger('BotController đã dừng');
  }

  setupApi() {
    this.app.use(express.json());
    this.app.post('/trade', async (req, res) => {
      const { action, tokenAddress, amount } = req.body;
      const tx = await this.executeTrade({ action, tokenAddress, amount });
      res.json(tx);
    });
    this.app.listen(3000, () => logger('API chạy trên port 3000'));
  }

  startWorker() {
    const workerData = {
      walletPrivateKey: this.transactionService.bot.walletPrivateKey,
      chains: this.transactionService.bot.chains,
      tslConfig: this.transactionService.bot.tslConfig,
      userId: this.transactionService.bot.userId
    };
    const worker = new Worker('./transactionWorker.js', { workerData });
    this.workers.push(worker);
    worker.on('message', msg => logger(`Worker: ${msg}`));
  }

  async executeTrade({ action, tokenAddress, amount }) {
    if (this.state !== 'running') throw new Error('Bot chưa chạy');
    return action === 'buy'
      ? await this.transactionService.buyToken(tokenAddress, amount)
      : await this.transactionService.sellToken(tokenAddress, amount);
  }
}

export default BotController;