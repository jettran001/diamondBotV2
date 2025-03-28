import React, { useState, useEffect } from 'react';
import { getTradeHistory } from '../services/api';

const HistoryPage = ({ userId }) => {
  const [history, setHistory] = useState([]);

  useEffect(() => {
    const fetchHistory = async () => {
      const trades = await getTradeHistory(userId);
      setHistory(trades);
    };
    fetchHistory();
  }, [userId]);

  return (
    <div style={{ padding: '20px', textAlign: 'center' }}>
      <h2>Trade History</h2>
      {history.length === 0 ? (
        <p>Chưa có giao dịch nào.</p>
      ) : (
        <table style={{ margin: '0 auto', borderCollapse: 'collapse' }}>
          <thead>
            <tr>
              <th style={{ border: '1px solid #ccc', padding: '5px' }}>Token</th>
              <th style={{ border: '1px solid #ccc', padding: '5px' }}>Action</th>
              <th style={{ border: '1px solid #ccc', padding: '5px' }}>Amount</th>
              <th style={{ border: '1px solid #ccc', padding: '5px' }}>Time</th>
            </tr>
          </thead>
          <tbody>
            {history.map((trade, index) => (
              <tr key={index}>
                <td style={{ border: '1px solid #ccc', padding: '5px' }}>{trade.token}</td>
                <td style={{ border: '1px solid #ccc', padding: '5px' }}>{trade.action}</td>
                <td style={{ border: '1px solid #ccc', padding: '5px' }}>{trade.amount}</td>
                <td style={{ border: '1px solid #ccc', padding: '5px' }}>{new Date(trade.time).toLocaleString()}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
};

export default HistoryPage;