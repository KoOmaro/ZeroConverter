import fs from 'fs';
import path from 'path';
import { pathToFileURL } from 'url';

const srcDir = path.join(process.cwd(), 'src');

/** Every source the bundler may load, TypeScript or JavaScript. */
export const SOURCE = /\.(ts|tsx|mts|cts|js|jsx|mjs|cjs)$/;

function walk(dir, callback) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    const fullPath = path.join(dir, file);
    if (fs.statSync(fullPath).isDirectory()) {
      walk(fullPath, callback);
    } else if (SOURCE.test(fullPath)) {
      callback(fullPath);
    }
  }
}

const normalizePath = (p) => p.replace(/\\/g, '/');

const rules = [
  {
    name: 'Rule 1: Components must not import Tauri',
    appliesTo: (p) => p.includes('/src/components/'),
    forbiddenPatterns: [
      { pattern: '@tauri-apps', name: '@tauri-apps' },
      { pattern: 'platform/tauri', name: 'platform/tauri' },
      { pattern: 'convertFileSrc', name: 'convertFileSrc' },
      { pattern: 'plugin-opener', name: 'plugin-opener' },
      { pattern: 'plugin-dialog', name: 'plugin-dialog' },
      { pattern: 'plugin-store', name: 'plugin-store' }
    ]
  },
  {
    name: 'Rule 2: Application layer must not import raw Tauri packages',
    appliesTo: (p) => p.includes('/src/application/'),
    forbiddenPatterns: [
      { pattern: '@tauri-apps', name: '@tauri-apps' }
    ]
  },
  {
    name: 'Rule 3: App shell must not import raw Tauri packages',
    appliesTo: (p) => p.endsWith('/src/App.tsx'),
    forbiddenPatterns: [
      { pattern: '@tauri-apps', name: '@tauri-apps' }
    ]
  },
  {
    name: 'Rule 4 & 5: Tauri Package & API Whitelists',
    appliesTo: (p) => true,
    forbiddenPatterns: [
      { pattern: '@tauri-apps/api/core', name: '@tauri-apps/api/core', 
        allowedIn: ['/src/platform/tauri/ipc.ts', '/src/platform/tauri/assetGateway.ts'] },
      { pattern: '@tauri-apps/api/window', name: '@tauri-apps/api/window', 
        allowedIn: ['/src/platform/tauri/windowGateway.ts'] },
      { pattern: '@tauri-apps/plugin-dialog', name: '@tauri-apps/plugin-dialog', 
        allowedIn: ['/src/platform/tauri/dialogGateway.ts'] },
      { pattern: '@tauri-apps/plugin-opener', name: '@tauri-apps/plugin-opener', 
        allowedIn: ['/src/platform/tauri/osGateway.ts'] },
      { pattern: '@tauri-apps/plugin-store', name: '@tauri-apps/plugin-store', 
        allowedIn: ['/src/platform/tauri/settingsGateway.ts'] },
      
      { pattern: 'convertFileSrc', name: 'convertFileSrc', 
        allowedIn: ['/src/platform/tauri/assetGateway.ts'] },
      { pattern: 'invoke(', name: 'invoke(', 
        allowedIn: ['/src/platform/tauri/ipc.ts'] },
      { pattern: 'import { invoke', name: 'import { invoke', 
        allowedIn: ['/src/platform/tauri/ipc.ts'] },
      { pattern: 'Channel<', name: 'Channel<', 
        allowedIn: ['/src/platform/tauri/ipc.ts', '/src/application/converterTypes.ts', '/src/platform/tauri/converterGateway.ts'] },
      { pattern: 'import { Channel', name: 'import { Channel', 
        allowedIn: ['/src/platform/tauri/ipc.ts', '/src/application/converterTypes.ts', '/src/platform/tauri/converterGateway.ts'] },
      { pattern: 'getCurrentWindow', name: 'getCurrentWindow', 
        allowedIn: ['/src/platform/tauri/windowGateway.ts'] },
      { pattern: 'LazyStore', name: 'LazyStore', 
        allowedIn: ['/src/platform/tauri/settingsGateway.ts'] },
      { pattern: 'revealItemInDir', name: 'revealItemInDir', 
        allowedIn: ['/src/platform/tauri/osGateway.ts'] },
      { pattern: 'openPath', name: 'openPath', 
        allowedIn: ['/src/platform/tauri/osGateway.ts'] }
    ]
  }
];

/** A comment line names things without importing them. */
const isComment = (line) => /^\s*(\/\/|\/\*|\*)/.test(line);

/** Rule breaks in one file; `normPath` uses forward slashes. */
export function violations(normPath, content) {
  const found = [];
  const lines = content.split('\n');

  for (const rule of rules) {
    if (rule.appliesTo(normPath)) {
      for (const forbidden of rule.forbiddenPatterns) {
        
        if (forbidden.allowedIn && forbidden.allowedIn.some(allowed => normPath.endsWith(allowed))) {
          continue;
        }

        for (let i = 0; i < lines.length; i++) {
          const line = lines[i];
          if (!isComment(line) && line.includes(forbidden.pattern)) {
            found.push({ rule: rule.name, line: i + 1, pattern: forbidden.name, text: line.trim() });
          }
        }
      }
    }
  }
  return found;
}

function main() {
  let failed = false;
  console.log("Scanning frontend boundaries...");
  walk(srcDir, (filePath) => {
    const normPath = normalizePath(filePath);
    for (const v of violations(normPath, fs.readFileSync(filePath, 'utf8'))) {
      console.error(`❌ [${v.rule}] Violation in ${normPath}:${v.line}`);
      console.error(`   Found forbidden pattern: "${v.pattern}"`);
      console.error(`   Line: ${v.text}`);
      failed = true;
    }
  });
  if (failed) {
    console.error("❌ Frontend boundary checks failed.");
    process.exit(1);
  }
  console.log("✅ All frontend boundary checks passed.");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
