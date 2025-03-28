import React, { useEffect, useState } from 'react';
import axios from 'axios';

const TelegramAuth = ({ onAuth }) => {
  const [jwtToken, setJwtToken] = useState('');

  useEffect(() => {
    const tg = window.Telegram.WebApp;
    tg.ready();
    const initData = tg.initDataUnsafe;
    axios.post('https://your-backend.com/login', {
      walletAddress: initData.user?.id || 'telegram_user',
      signature: 'telegram_signature' // Thay bằng logic thực tế
    }).then(res => {
      setJwtToken(res.data.token);
      onAuth(res.data.token);
    });
  }, [onAuth]);

  return null; // Không hiển thị UI
};

export default TelegramAuth;