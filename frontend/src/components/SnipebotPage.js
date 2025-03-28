import React, { useState } from 'react';
import { evaluateToken, manualTrade, autoTrade, upgradeRole } from '../services/api';

const SnipebotPage = ({ userId }) => {
  const [mode, setMode] = useState(null); // 'manual' hoặc 'auto'
  const [tokenInput, setTokenInput] = useState('');
  const [tokenLog, setTokenLog] = useState(null);
  const [amount, setAmount] = useState('0.1');
  const [customGas, setCustomGas] = useState('');
  const [autoRole, setAutoRole] = useState('free');
  const [paymentTx, setPaymentTx] = useState('');

  const handleManualSubmit = async () => {
    const log = await evaluateToken(userId, tokenInput);
    setTokenLog(log);
  };

  const handleManualTrade = async (action) => {
    const result = await manualTrade(userId, tokenLog.tokenInfo.address, amount, action, customGas ? { gasPrice: customGas } : null);
    alert(`Giao dịch ${action}: ${JSON.stringify(result)}`);
  };

  const handleAutoStart = async () => {
    const user = await upgradeRole(userId, autoRole, paymentTx);
    if (user.role === autoRole) {
      const result = await autoTrade(userId, '0xTokenAddress', amount, 'buy'); // Ví dụ token
      alert(`Auto trade: ${JSON.stringify(result)}`);
    }
  };

  return (
    <div style={{ padding: '20px', textAlign: 'center' }}>
      <h2>Snipebot Trading</h2>
      <div style={{ display: 'flex', justifyContent: 'center', gap: '20px' }}>
        <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
          <h3>Manual Mode</h3>
          <button onClick={() => setMode('manual')}>Start Manual</button>
        </div>
        <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
          <h3>Auto Mode</h3>
          <button onClick={() => setMode('auto')}>Start Auto</button>
        </div>
      </div>

      {mode === 'manual' && (
        <div style={{ marginTop: '20px' }}>
          <input
            type="text"
            value={tokenInput}
            onChange={(e) => setTokenInput(e.target.value)}
            placeholder="Token address, name, or symbol"
            style={{ padding: '5px', width: '300px' }}
          />
          <button onClick={handleManualSubmit} style={{ marginLeft: '10px' }}>Submit</button>
          {tokenLog && (
            <div style={{ marginTop: '10px', textAlign: 'left', maxWidth: '500px', margin: '10px auto' }}>
              <pre>{JSON.stringify(tokenLog, null, 2)}</pre>
              <button onClick={() => handleManualTrade('buy')}>Buy</button>
              <button onClick={() => handleManualTrade('sell')} style={{ marginLeft: '10px' }}>Sell</button>
              <input
                type="text"
                value={customGas}
                onChange={(e) => setCustomGas(e.target.value)}
                placeholder="Custom Gas (Gwei)"
                style={{ marginLeft: '10px', padding: '5px' }}
              />
            </div>
          )}
        </div>
      )}

      {mode === 'auto' && (
        <div style={{ marginTop: '20px' }}>
          <div style={{ display: 'flex', justifyContent: 'center', gap: '20px' }}>
            <div style={{ border: '1px solid #ccc', padding: '20px' }}>
              <h4>Free</h4>
              <button onClick={() => { setAutoRole('free'); handleAutoStart(); }}>Start Free</button>
            </div>
            <div style={{ border: '1px solid #ccc', padding: '20px' }}>
              <h4>Premium</h4>
              <button onClick={() => setAutoRole('premium')}>Start Premium</button>
            </div>
            <div style={{ border: '1px solid #ccc', padding: '20px' }}>
              <h4>Ultimate</h4>
              <button onClick={() => setAutoRole('ultimate')}>Start Ultimate</button>
            </div>
          </div>
          {autoRole !== 'free' && (
            <div style={{ marginTop: '20px' }}>
              <input
                type="text"
                value={paymentTx}
                onChange={(e) => setPaymentTx(e.target.value)}
                placeholder="Payment Transaction Hash"
                style={{ padding: '5px', width: '300px' }}
              />
              <button onClick={handleAutoStart} style={{ marginLeft: '10px' }}>Confirm Payment</button>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

export default SnipebotPage;