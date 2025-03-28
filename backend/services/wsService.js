import { WebSocketServer } from 'ws';
import jwt from 'jsonwebtoken';
import { createClient } from 'redis';
import logger from '../utils/logger.js';
import http from 'http';

class WsService {
  static instance;

  constructor({ maxClients = 100, port = process.env.WS_PORT || 9000, retryInterval = 5000 }) {
    if (WsService.instance) return WsService.instance;
    WsService.instance = this;

    this.maxClients = maxClients;
    this.retryInterval = retryInterval;
    this.port = parseInt(port);
    this.redis = createClient({
      url: process.env.REDIS_URL || 'redis://redis:6379',
      retry_strategy: (options) => {
        if (options.attempt > 5) return new Error('Redis retry limit reached'); // Giới hạn 5 lần
        return 5000;
      }
    });
    this.wss = null;
    this.server = null;
    this.startServer();
  }

  async startServer() {
    this.server = http.createServer((req, res) => {
      if (req.url === '/health') {
        res.writeHead(200, { 'Content-Type': 'text/plain' });
        res.end('OK');
      } else {
        res.writeHead(404);
        res.end();
      }
    });

    try {
      await new Promise((resolve, reject) => {
        this.server.listen(this.port, () => {
          logger(`HTTP server running on port ${this.port}`, 'info'); // Log 1 lần
          resolve();
        }).on('error', (err) => {
          reject(err);
        });
      });
    } catch (err) {
      logger(`Failed to start HTTP server on port ${this.port}: ${err.message}`, 'error');
      process.exit(1);
    }

    this.wss = new WebSocketServer({ server: this.server });
    logger(`WebSocket server started on port ${this.port}`, 'info');
    this.setupConnection();

    try {
      await this.redis.connect();
      logger(`Redis connected on ${this.redis.options.url}`, 'info');
      await this.redis.set('ws_port', this.port, { NX: true });
    } catch (err) {
      logger(`Redis connect error: ${err.message}`, 'warn');
    }
  }

  setupConnection() {
    this.wss.on('connection', (ws) => {
      if (this.wss.clients.size >= this.maxClients) {
        ws.close(1008, 'Server full');
        logger('Connection rejected: client limit reached', 'warn');
        return;
      }

      ws.on('message', async (msg) => {
        try {
          const data = JSON.parse(msg);
          if (data.type === 'broadcast') {
            this.wss.clients.forEach(client => client.send(data.message));
            return;
          }
          const { token } = data;
          const decoded = jwt.verify(token, process.env.JWT_SECRET || 'your-secret-key');
          ws.send(JSON.stringify({ status: 'Connected', userId: decoded.userId }));
          logger(`WebSocket client connected: ${decoded.userId}`, 'info');
        } catch (err) {
          ws.close(1008, 'Invalid token');
          logger(`WebSocket auth error: ${err.message}`, 'error');
        }
      });

      ws.on('close', () => logger('WebSocket client disconnected', 'info'));
    });
  }

  async close() {
    if (this.wss) this.wss.close();
    if (this.server) this.server.close();
    await this.redis.quit();
    logger('WebSocket server closed', 'info');
  }
}

const wsService = new WsService({});
export default wsService;