import { parentPort, workerData } from 'worker_threads';
import TransactionService from './services/transactionService.js';

const { walletPrivateKey, chains, tslConfig, userId } = workerData;
const botConfig = { walletPrivateKey, chains, tslConfig, userId };
const service = new TransactionService(botConfig);

// Danh sách token để giao dịch tự động (có thể cấu hình qua workerData sau)
const autoTradeConfig = workerData.autoTradeConfig ||{
  tokenAddress: '0xTokenExample', // Thay bằng token thực tế
  amount: '0.1', // Số lượng mặc định
  interval: 60000 // 1 phút
};

(async () => {
  try {
    await service.initialize();
    parentPort.postMessage('Worker đã khởi động');

    // Logic giao dịch tự động
    setInterval(async () => {
      try {
        const tx = await service.buyToken(autoTradeConfig.tokenAddress, autoTradeConfig.amount);
        parentPort.postMessage(`Auto trade thành công: ${JSON.stringify(tx)}`);
      } catch (error) {
        parentPort.postMessage(`Auto trade lỗi: ${error.message}`);
      }
    }, autoTradeConfig.interval);

  } catch (error) {
    parentPort.postMessage(`Worker lỗi: ${error.message}`);
    process.exit(1);
  }
})();