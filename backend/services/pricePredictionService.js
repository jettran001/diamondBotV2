import * as tf from '@tensorflow/tfjs-node';
import axios from 'axios';
import logger from '../utils/logger.js';

class PricePredictionService {
  constructor() {
    this.model = null;
  }

  async initialize() {
    this.model = tf.sequential();
    this.model.add(tf.layers.lstm({ units: 100, inputShape: [10, 3], returnSequences: false })); // 3 đặc trưng: price, volume, liquidity
    this.model.add(tf.layers.dense({ units: 1 }));
    this.model.compile({ optimizer: tf.train.adam(0.001), loss: 'meanSquaredError' }); // Tuning learning rate
    await logger('PricePredictionService đã khởi tạo');
  }

  async fetchRealTimeData(tokenId) {
    const response = await axios.get(`https://api.coingecko.com/api/v3/coins/${tokenId}/market_chart?vs_currency=usd&days=1`);
    return response.data.prices.map(([time, price]) => ({
      time,
      price,
      volume: response.data.total_volumes.find(v => v[0] === time)[1],
      liquidity: price * 0.1 // Giả lập liquidity
    }));
  }

  async train(tokenId) {
    const data = await this.fetchRealTimeData(tokenId);
    const xs = tf.tensor3d(data.map(d => [d.price, d.volume, d.liquidity].slice(0, 10)), [data.length - 1, 10, 3]);
    const ys = tf.tensor2d(data.slice(1).map(d => [d.price]), [data.length - 1, 1]);
    await this.model.fit(xs, ys, { epochs: 20, batchSize: 32 });
    await logger('Đã huấn luyện mô hình dự đoán giá');
    xs.dispose();
    ys.dispose();
  }

  async predict(history) {
    const input = tf.tensor3d([history.slice(-10).map(d => [d.price, d.volume, d.liquidity])], [1, 10, 3]);
    const prediction = this.model.predict(input);
    const result = prediction.dataSync()[0];
    await logger(`Dự đoán giá: ${result}`);
    input.dispose();
    prediction.dispose();
    return result;
  }
}

export default PricePredictionService;