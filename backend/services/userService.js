import logger from '../utils/logger.js';
import { ethers } from 'ethers';
import config from '../config/config.js';

class UserService {
  constructor() {
    this.users = new Map(); // { userId: { role, turnsLeft, lastReset } }
    this.reserveWallet = config.reserveWallet;
  }

  async checkRole(userId) {
    const user = this.users.get(userId) || { role: 'free', turnsLeft: 1, lastReset: Date.now() };
    this.resetTurnsIfNeeded(user);
    this.users.set(userId, user);
    return user;
  }

  async useTurn(userId) {
    const user = await this.checkRole(userId);
    if (user.turnsLeft <= 0) throw new Error('Hết lượt sử dụng');
    user.turnsLeft -= 1;
    this.users.set(userId, user);
    await logger(`User ${userId} sử dụng 1 turn, còn ${user.turnsLeft} turn`);
  }

  resetTurnsIfNeeded(user) {
    const now = Date.now();
    if (now - user.lastReset > 24 * 60 * 60 * 1000) { // 24h
      user.turnsLeft = this.getMaxTurns(user.role);
      user.lastReset = now;
      logger(`Reset turn cho user ${user.role} lúc ${new Date(now).toUTCString()}`);
    }
  }

  getMaxTurns(role) {
    return { free: 1, premium: 12, ultimate: 24 }[role];
  }

  async upgradeRole(userId, targetRole, paymentTx) {
    const user = await this.checkRole(userId);
    const paymentAmount = this.getUpgradeCost(user.role, targetRole);
    const provider = ethers.getDefaultProvider('https://eth.llamarpc.com');
    const tx = await provider.getTransaction(paymentTx);
    const isDMD = tx.data.includes('0xa9059cbb'); // Giả sử DMD là ERC-20 token
    const amount = isDMD ? ethers.utils.hexToNumberString(tx.data.slice(74)) / 1e18 : ethers.formatEther(tx.value);

    if (tx.to !== this.reserveWallet || amount < paymentAmount) {
      throw new Error('Thanh toán không hợp lệ');
    }
    user.role = targetRole;
    user.turnsLeft = this.getMaxTurns(targetRole);
    this.users.set(userId, user);
    await logger(`User ${userId} nâng cấp lên ${targetRole}`);
    return user;
  }

  getUpgradeCost(currentRole, targetRole) {
    if (targetRole === 'premium') return 300;
    if (targetRole === 'ultimate') return currentRole === 'premium' ? 700 : 1000;
    return 0;
  }
}

export default UserService;