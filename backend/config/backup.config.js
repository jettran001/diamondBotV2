const config = {
  ignorePatterns: [
    'node_modules',
    'dist',
    'build',
    'logs',
    'temp',
    'Memory',
    'package-lock.json',
    '.env',
    '.git',
    'project_log.json'
  ],
  includePatterns: ['src', 'utils', 'config'],
  backupDir: 'Memory'
};

export default config; // Export mặc định cho ESM