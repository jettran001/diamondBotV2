import { promises as fs } from 'fs';
import { createReadStream } from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// Xác định __dirname trong ESM
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Đường dẫn thư mục gốc của dự án (lên 2 cấp từ backend/utils đến Diamond_Snipebot_v2.0)
const projectRoot = path.resolve(__dirname, '..', '..');

// Cấu hình sao lưu
const config = {
  ignorePatterns: ['node_modules','Memory'], // Chỉ bỏ qua node_modules
  backupDir: 'Memory', // Thư mục lưu file sao lưu
  maxFileSize: 10 * 1024 * 1024 // Giới hạn kích thước file: 10MB (tùy chỉnh được)
};

// Hàm lấy ngày hiện tại (YYYY-MM-DD_HH-MM-SS) để tạo tên file độc nhất mỗi lần chạy
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

// Hàm quét tất cả file trong thư mục
async function getAllFiles(dir, fileList = [], relativePath = '') {
  try {
    const files = await fs.readdir(dir, { withFileTypes: true });
    for (const file of files) {
      const filePath = path.join(dir, file.name);
      const relativeFilePath = path.join(relativePath, file.name).replace(/\\/g, '/');

      // Bỏ qua các thư mục/file trong ignorePatterns
      if (config.ignorePatterns.some(pattern => filePath.includes(pattern) || file.name === pattern)) {
        continue;
      }

      if (file.isDirectory()) {
        await getAllFiles(filePath, fileList, relativeFilePath);
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

// Hàm đọc nội dung file qua stream
async function readFileContent(filePath) {
  try {
    const stats = await fs.stat(filePath);
    if (stats.size > config.maxFileSize) {
      return `// Lỗi: File quá lớn (${(stats.size / 1024 / 1024).toFixed(2)}MB) - Giới hạn: ${config.maxFileSize / 1024 / 1024}MB`;
    }

    const stream = createReadStream(filePath, 'utf-8');
    let content = '';
    
    for await (const chunk of stream) {
      content += chunk;
    }
    
    return content;
  } catch (error) {
    return `// Lỗi: Không thể đọc nội dung - ${error.message}`;
  }
}

// Hàm tạo thư mục backupDir nếu chưa tồn tại
async function ensureBackupDir() {
  const backupPath = path.join(projectRoot, config.backupDir);
  try {
    await fs.mkdir(backupPath, { recursive: true });
  } catch (error) {
    throw new Error(`Lỗi tạo thư mục ${backupPath}: ${error.message}`);
  }
}

// Hàm chính để tạo file sao lưu
async function backupProject() {
  try {
    console.log('Bắt đầu sao lưu dự án...');

    // Quét tất cả file
    const files = await getAllFiles(projectRoot);
    console.log(`Tìm thấy ${files.length} file.`);

    // Đọc nội dung từng file qua stream
    const projectLog = [];
    for (const file of files) {
      const fullPath = path.join(projectRoot, file.filePath);
      const content = await readFileContent(fullPath);
      projectLog.push({
        fileName: file.fileName,
        filePath: file.filePath,
        content: content
      });
    }

    // Tạo tên file với timestamp
    const timestamp = getTimestamp();
    const logFileName = `project_log_${timestamp}.json`;
    const logFilePath = path.join(projectRoot, config.backupDir, logFileName);

    // Ghi file sao lưu
    await ensureBackupDir();
    await fs.writeFile(logFilePath, JSON.stringify(projectLog, null, 2));
    console.log(`Đã tạo file sao lưu tại ${logFilePath}`);
  } catch (error) {
    console.error(`Lỗi sao lưu dự án: ${error.message}`);
  }
}

// Chạy sao lưu ngay khi script được gọi
backupProject();