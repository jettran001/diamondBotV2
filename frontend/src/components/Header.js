import React from 'react';

const Header = () => {
  return (
    <header style={{ padding: '10px', background: '#1a1a1a', color: '#fff', textAlign: 'center' }}>
      <h1>Diamond Snipebot v2.0</h1>
      <nav>
        <a href="/" style={{ color: '#fff', margin: '0 10px' }}>Home</a>
        <a href="/settings" style={{ color: '#fff', margin: '0 10px' }}>Settings</a>
        <a href="/history" style={{ color: '#fff', margin: '0 10px' }}>History</a>
      </nav>
    </header>
  );
};

export default Header;