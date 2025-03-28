import redis from 'redis';

const redisClient = redis.createClient({
  url: process.env.REDIS_URL || 'redis://redis:6379',
});

redisClient.on('error', (err) => {
  console.error('Redis connect error:', err);
});

await redisClient.connect();

export default redisClient;