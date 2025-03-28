// backend/utils/helpers.js
import { createServer } from 'net';
import { ethers } from 'ethers';
import logger from './logger.js';

async function isPortTaken(port) {
  return new Promise((resolve) => {
    const server = createServer();
    server.once('error', (err) => {
      if (err.code === 'EADDRINUSE') resolve(true);
      else resolve(false);
    });
    server.once('listening', () => {
      server.close();
      resolve(false);
    });
    server.listen(Number(port));
  });
}

function formatNumber(num, decimals = 2) {
  return parseFloat(num).toFixed(decimals);
}

async function signMessage(message, privateKey) {
  const wallet = new ethers.Wallet(privateKey);
  return wallet.signMessage(message);
}

function isValidAddress(address) {
  return ethers.isAddress(address);
}

function convertTimestamp(timestamp) {
  return new Date(timestamp * 1000).toISOString();
}

function validateInput(data, schema) {
  for (const [key, type] of Object.entries(schema)) {
    if (typeof data[key] !== type) {
      logger(`Dữ liệu không hợp lệ: ${key} phải là ${type}`, 'error');
      throw new Error(`Invalid ${key}: expected ${type}`);
    }
  }
  return true;
}

export { formatNumber, signMessage, isValidAddress, convertTimestamp, validateInput, isPortTaken };