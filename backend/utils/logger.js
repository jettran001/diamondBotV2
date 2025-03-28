import winston from 'winston';
import fs from 'fs';
import path from 'path';
import redisClient from './redisClient.js';

// Đảm bảo thư mục logs tồn tại
const logDir = 'logs';
if (!fs.existsSync(logDir)) {
  fs.mkdirSync(logDir);
}

// Lấy logLevel từ biến môi trường
const logLevel = process.env.LOG_LEVEL || 'info';

// Tạo logger với Winston
const logger = winston.createLogger({
  levels: { error: 0, warn: 1, info: 2 },
  level: logLevel,
  format: winston.format.combine(
    winston.format.timestamp({ format: 'YYYY-MM-DD HH:mm:ss' }),
    winston.format.printf(({ timestamp, level, message }) => `[${timestamp}] [${level.toUpperCase()}] ${message}`)
  ),
  transports: [
    new winston.transports.Console({
      format: winston.format.combine(
        winston.format.colorize(),
        winston.format.simple()
      ),
    }),
    new winston.transports.File({
      filename: path.join(logDir, process.env.LOG_PATH || 'server.log'),
      maxsize: 10 * 1024 * 1024, // 10MB
      maxFiles: 5,
      tailable: true,
    }),
  ],
  exceptionHandlers: [
    new winston.transports.File({ filename: path.join(logDir, 'exceptions.log') })
  ],
  rejectionHandlers: [
    new winston.transports.File({ filename: path.join(logDir, 'rejections.log') })
  ],
});

// Export hàm log đơn giản, tránh vòng lặp log
export default async function log(message, level = 'info') {
  const logEntry = {
    timestamp: new Date().toISOString(),
    level,
    message
  };

  // Ghi log bằng Winston trước
  logger.log({ level, message });

  // Đẩy log vào Redis nếu là error hoặc warn, nhưng không ghi log lỗi Redis vào Winston
  if (level === 'error' || level === 'warn') {
    try {
      await redisClient.lPush('logs', JSON.stringify(logEntry));
    } catch (err) {
      // Ghi lỗi Redis trực tiếp vào file, không dùng console.error để tránh vòng lặp
      const errorMessage = `[${new Date().toISOString()}] [ERROR] Redis lPush error: ${err.message}\n`;
      fs.appendFileSync(path.join(logDir, 'redis_errors.log'), errorMessage);
    }
  }
}