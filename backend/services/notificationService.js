import logger from '../utils/logger.js';
import { createClient } from 'redis';
import axios from 'axios';

class NotificationService {
  constructor({ telegramToken, discordWebhook, broadcast }) {
    this.redis = createClient({
      url: process.env.REDIS_URL || 'redis://redis:6379',
      retry_strategy: () => ({
        attempts: parseInt(process.env.REDIS_RETRY_ATTEMPTS) || 10,
        delay: parseInt(process.env.REDIS_RETRY_DELAY) || 5000
      })
    });
    this.telegramToken = telegramToken;
    this.discordWebhook = discordWebhook;
    this.broadcast = broadcast;
  }

  async initialize() {
    await this.redis.connect();
    await logger('NotificationService initialized', 'info');
  }

  async sendNotification(message) {
    const cached = await this.redis.get(`notification:${message}`);
    if (cached) return;
    await this.redis.setEx(`notification:${message}`, 3600, message); // Cache 1 giờ
    await this.redis.lPush('notifications', message);
    this.broadcast(message);
    if (this.telegramToken) await this.sendTelegram(message);
    if (this.discordWebhook) await this.sendDiscord(message);
    await logger(`Notification sent: ${message}`, 'info');
  }

  async sendTelegram(message) {
    await axios.post(`https://api.telegram.org/bot${this.telegramToken}/sendMessage`, {
      chat_id: process.env.TELEGRAM_CHAT_ID,
      text: message
    });
  }

  async sendDiscord(message) {
    await axios.post(this.discordWebhook, { content: message });
  }

  async close() {
    await this.redis.quit();
    await logger('NotificationService closed', 'info');
  }
}

export default NotificationService;