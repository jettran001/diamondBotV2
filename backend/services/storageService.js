import { MongoClient } from 'mongodb';
import { create } from 'ipfs-http-client';
import { createClient } from 'redis';
import logger from '../utils/logger.js';
import { createCipheriv, createDecipheriv, randomBytes } from 'crypto';

class StorageService {
  constructor({
    mongoUrl = process.env.MONGO_URL || 'mongodb://localhost:27017',
    ipfsUrl = `${process.env.IPFS_PROTOCOL || 'https'}://${process.env.IPFS_HOST || 'ipfs.infura.io'}:${process.env.IPFS_PORT || 5001}`,
    redisUrl = process.env.REDIS_URL || 'redis://localhost:6379'
  }) {
    this.mongoUrl = mongoUrl;
    this.ipfsUrl = ipfsUrl;
    this.redisUrl = redisUrl;
    this.db = null;
    this.ipfs = null;
    this.redis = null;
    this.encryptionKey = process.env.ENCRYPTION_KEY || randomBytes(32); // 32 bytes cho AES-256
    this.iv = randomBytes(16); // Initialization vector
  }

  async initialize() {
    const client = new MongoClient(this.mongoUrl);
    await client.connect();
    this.db = client.db('diamond_snipebot');
    this.ipfs = create({ url: this.ipfsUrl });
    this.redis = createClient({ url: this.redisUrl });
    await this.redis.connect();
    await logger('StorageService đã khởi tạo');
  }

  async saveTransaction(txData) {
    const cipher = createCipheriv('aes-256-cbc', this.encryptionKey, this.iv);
    let encryptedData = cipher.update(JSON.stringify(txData), 'utf8', 'hex');
    encryptedData += cipher.final('hex');
    const cid = await this.ipfs.add(encryptedData);
    const result = await this.db.collection('transactions').insertOne({ ...txData, ipfsCid: cid.path });
    await this.redis.set(`tx:${result.insertedId}`, JSON.stringify(txData), { EX: 3600 }); // Cache 1 giờ
    await logger(`Lưu giao dịch ${result.insertedId} vào MongoDB và IPFS: ${cid.path}`);
    return { mongoId: result.insertedId, ipfsCid: cid.path };
  }

  async getTransaction(txId) {
    const cached = await this.redis.get(`tx:${txId}`);
    if (cached) {
      await logger(`Đọc giao dịch ${txId} từ Redis`);
      return JSON.parse(cached);
    }
    const tx = await this.db.collection('transactions').findOne({ _id: txId });
    if (tx) {
      await this.redis.set(`tx:${txId}`, JSON.stringify(tx), { EX: 3600 });
      await logger(`Đọc giao dịch ${txId} từ MongoDB`);
      return tx;
    }
    return null;
  }

  async getFromIPFS(cid) {
    const stream = this.ipfs.cat(cid);
    let encryptedData = '';
    for await (const chunk of stream) {
      encryptedData += chunk.toString();
    }
    const decipher = createDecipheriv('aes-256-cbc', this.encryptionKey, this.iv);
    let decryptedData = decipher.update(encryptedData, 'hex', 'utf8');
    decryptedData += decipher.final('utf8');
    return JSON.parse(decryptedData);
  }

  async close() {
    await this.db.client.close();
    await this.redis.quit();
    await logger('StorageService đã đóng');
  }
}

export default StorageService;