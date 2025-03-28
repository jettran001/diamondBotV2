import { promises as fs } from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// Xác định __dirname trong ESM
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Đường dẫn thư mục gốc của dự án
const projectRoot = path.resolve(__dirname, '..', '..');

// Cấu hình sao lưu
const config = {
  ignorePatterns: ['node_modules', 'Memory', 'dist', 'build', 'logs', '*.log','.git','package-lock.json'],
  backupDir: 'Memory'
};

// Hàm lấy timestamp (YYYY-MM-DD_HH-MM-SS)
function getTimestamp() {
  const now = new Date();
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, '0');
  const day = String(now.getDate()).padStart(2, '0');
  const hours = String(now.getHours()).padStart(2, '0');
  const minutes = String(now.getMinutes()).padStart(2, '0');
  const seconds = String(now.getSeconds()).padStart(2, '0');
  return `${year}-${month}-${day}_${hours}-${minutes}-${seconds}`;
}

// Hàm retry khi đọc file
async function retry(fn, retries = 3, delay = 1000) {
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      return await fn();
    } catch (error) {
      if (attempt === retries) return `// Lỗi sau ${retries} lần thử: ${error.message}`;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}

// Kiểm tra file text
const textFileExtensions = ['.js', '.json', '.md', '.txt', '.html', '.css'];
const isTextFile = (fileName) => textFileExtensions.some(ext => fileName.endsWith(ext));

// Hàm quét tất cả file
async function getAllFiles(dir, fileList = []) {
  try {
    const files = await fs.readdir(dir, { withFileTypes: true });
    for (const file of files) {
      const filePath = path.join(dir, file.name);
      const relativeFilePath = path.relative(projectRoot, filePath).replace(/\\/g, '/');

      if (config.ignorePatterns.some(pattern => filePath.includes(pattern) || file.name === pattern)) {
        continue;
      }

      if (file.isDirectory()) {
        await getAllFiles(filePath, fileList);
      } else {
        fileList.push({
          fileName: file.name,
          filePath: relativeFilePath
        });
      }
    }
    return fileList;
  } catch (error) {
    throw new Error(`Lỗi quét thư mục ${dir}: ${error.message}`);
  }
}

// Hàm đọc nội dung file
async function readFileContent(filePath) {
  const fileName = path.basename(filePath);
  if (isTextFile(fileName)) {
    return await retry(() => fs.readFile(filePath, 'utf-8'));
  } else {
    return '// File nhị phân, không đọc nội dung';
  }
}

// Hàm tạo thư mục sao lưu
async function ensureBackupDir(timestamp) {
  const backupPath = path.join(projectRoot, config.backupDir, `backup_${timestamp}`);
  try {
    await fs.mkdir(backupPath, { recursive: true });
    return backupPath;
  } catch (error) {
    throw new Error(`Lỗi tạo thư mục ${backupPath}: ${error.message}`);
  }
}

// Hàm chính để tạo file sao lưu
async function backupProject() {
  try {
    console.log('Bắt đầu sao lưu dự án...');

    const files = await getAllFiles(projectRoot);
    console.log(`Tìm thấy ${files.length} file.`);

    const projectLog = [];
    for (const file of files) {
      const fullPath = path.join(projectRoot, file.filePath);
      const content = await readFileContent(fullPath);
      projectLog.push({
        fileName: file.fileName,
        filePath: file.filePath,
        content: content
      });
      console.log(`Đã sao lưu: ${file.filePath}`);
    }

    const timestamp = getTimestamp();
    const backupPath = await ensureBackupDir(timestamp);
    const logFilePath = path.join(backupPath, `project_log_${timestamp}.json`);

    await fs.writeFile(logFilePath, JSON.stringify(projectLog, null, 2));
    console.log(`Đã tạo file sao lưu tại ${logFilePath}`);
  } catch (error) {
    console.error(`Lỗi sao lưu dự án: ${error.message}`);
  }
}

// Chạy sao lưu
backupProject();