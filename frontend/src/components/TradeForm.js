import React, { useState, useEffect } from 'react';
import axios from 'axios';
import { w3cwebsocket as W3CWebSocket } from 'ws';

const TradeForm = ({ jwtToken }) => {
  const [tokenAddress, setTokenAddress] = useState('');
  const [amount, setAmount] = useState('');
  const [action, setAction] = useState('buy');
  const [status, setStatus] = useState('');
  const [balance, setBalance] = useState(null);

  useEffect(() => {
    // Kết nối WebSocket từ notificationService.js
    const ws = new W3CWebSocket('wss://your-backend.com:8080');
    ws.onopen = () => ws.send(JSON.stringify({ token: 'VALID_TOKEN' }));
    ws.onmessage = (msg) => setStatus(`Update: ${msg.data}`);
    return () => ws.close();
  }, []);

  const checkBalance = async () => {
    const res = await axios.get('https://your-backend.com/balance', {
      headers: { Authorization: `Bearer ${jwtToken}` }
    });
    setBalance(res.data.balance);
  };

  const handleTrade = async () => {
    try {
      const res = await axios.post('https://your-backend.com/trade', {
        action, tokenAddress, amount
      }, { headers: { Authorization: `Bearer ${jwtToken}` } });
      setStatus(`Trade ${action} thành công: ${JSON.stringify(res.data)}`);
    } catch (error) {
      setStatus(`Lỗi: ${error.response?.data?.error || error.message}`);
    }
  };

  return (
    <div>
      <input value={tokenAddress} onChange={e => setTokenAddress(e.target.value)} placeholder="Token Address" />
      <input value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount" />
      <select value={action} onChange={e => setAction(e.target.value)}>
        <option value="buy">Buy</option>
        <option value="sell">Sell</option>
      </select>
      <button onClick={handleTrade}>Execute</button>
      <button onClick={checkBalance}>Check Balance</button>
      <p>Balance: {balance || 'N/A'}</p>
      <p>{status}</p>
    </div>
  );
};

export default TradeForm;