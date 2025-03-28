import React, { useState, useEffect } from 'react';
import { BrowserRouter as Router, Route, Routes } from 'react-router-dom';
import Header from './components/Header';
import Footer from './components/Footer';
import SnipebotPage from './components/SnipebotPage';
import SettingsPage from './components/SettingsPage';
import HistoryPage from './components/HistoryPage';
import TelegramAuth from './components/TelegramAuth';
import TradeForm from './components/TradeForm';

const App = () => {
  const [jwtToken, setJwtToken] = useState('');
  const [isTelegram, setIsTelegram] = useState(false);
  const [theme, setTheme] = useState({});
  const userId = 'user123'; // Thay bằng logic thực tế sau

  useEffect(() => {
    // Kiểm tra nếu chạy trong Telegram Mini App
    if (window.Telegram?.WebApp?.initDataUnsafe) {
      setIsTelegram(true);
      const tg = window.Telegram.WebApp;
      tg.ready();
      tg.expand();
      setTheme(tg.themeParams); // Đồng bộ theme Telegram
    }
  }, []);

  return (
    <div className="app" style={{ backgroundColor: theme.bg_color || '#fff', color: theme.text_color || '#000' }}>
      {isTelegram ? (
        <>
          <h1>Diamond SnipeBot</h1>
          <TelegramAuth onAuth={setJwtToken} />
          {jwtToken && <TradeForm jwtToken={jwtToken} />}
        </>
      ) : (
        <Router>
          <Header />
          <Routes>
            <Route path="/" element={<SnipebotPage userId={userId} />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/history" element={<HistoryPage userId={userId} />} />
          </Routes>
          <Footer />
        </Router>
      )}
    </div>
  );
};

export default App;