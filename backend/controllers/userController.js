import StorageService from '../services/storageService.js';
import logger from '../utils/logger.js';
import jwt from 'jsonwebtoken';
import { ethers } from 'ethers';

class UserController {
  constructor() {
    this.storageService = new StorageService({});
    this.secret = 'YOUR_JWT_SECRET';
  }

  async initialize() {
    await this.storageService.initialize();
    await logger('UserController đã khởi tạo');
  }

  async registerUser(walletAddress, signature) {
    const message = 'Sign to register';
    const signer = ethers.verifyMessage(message, signature);
    if (signer !== walletAddress) throw new Error('Chữ ký không hợp lệ');
    const user = { walletAddress, role: 'user', createdAt: new Date() };
    const result = await this.storageService.saveTransaction(user);
    const token = jwt.sign({ walletAddress, role: 'user' }, this.secret, { expiresIn: '1h' });
    await logger(`Đã đăng ký người dùng: ${walletAddress}`);
    return { ...result, token };
  }

  async login(walletAddress, signature) {
    const user = await this.getUser(walletAddress);
    if (!user) throw new Error('Người dùng không tồn tại');
    const signer = ethers.verifyMessage('Sign to login', signature);
    if (signer !== walletAddress) throw new Error('Chữ ký không hợp lệ');
    const token = jwt.sign({ walletAddress, role: user.role }, this.secret, { expiresIn: '1h' });
    await logger(`Đã đăng nhập: ${walletAddress}`);
    return { token };
  }

  async getUser(walletAddress) {
    const user = await this.storageService.getTransaction(walletAddress);
    return user;
  }

  verifyToken(token) {
    return jwt.verify(token, this.secret);
  }

  hasPermission(token, requiredRole) {
    const decoded = this.verifyToken(token);
    return decoded.role === requiredRole || decoded.role === 'admin';
  }
}

export default UserController;