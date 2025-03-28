import axios from 'axios';

const API_URL = process.env.REACT_APP_API_URL || 'https://your-backend.com';

export const evaluateToken = async (userId, tokenInput) => {
  const response = await axios.post(`${API_URL}/evaluate`, { userId, tokenInput });
  return response.data;
};

export const manualTrade = async (userId, tokenAddress, amount, action, customGas) => {
  const response = await axios.post(`${API_URL}/manual-trade`, { userId, tokenAddress, amount, action, customGas });
  return response.data;
};

export const autoTrade = async (userId, tokenAddress, amount, action) => {
  const response = await axios.post(`${API_URL}/auto-trade`, { userId, tokenAddress, amount, action });
  return response.data;
};

export const upgradeRole = async (userId, role, paymentTx) => {
  const response = await axios.post(`${API_URL}/upgrade-role`, { userId, role, paymentTx });
  return response.data;
};

export const getTradeHistory = async (userId) => {
  const response = await axios.get(`${API_URL}/history`, { params: { userId } });
  return response.data;
};

export const getBalance = async (jwtToken) => {
  const response = await axios.get(`${API_URL}/balance`, {
    headers: { Authorization: `Bearer ${jwtToken}` }
  });
  return response.data;
};