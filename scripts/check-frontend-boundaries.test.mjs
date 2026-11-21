import test from 'node:test';
import assert from 'node:assert/strict';
import { SOURCE, violations } from './check-frontend-boundaries.mjs';

test('JavaScript sources are scanned too', () => {
  for (const name of ['a.ts', 'a.tsx', 'a.js', 'a.jsx', 'a.mjs', 'a.mts', 'a.cjs']) {
    assert.ok(SOURCE.test(name), name);
  }
  assert.ok(!SOURCE.test('a.css'));
});

test('a component may not import Tauri', () => {
  const found = violations(
    '/repo/src/components/Button.jsx',
    "import { invoke } from '@tauri-apps/api/core';\n",
  );
  assert.ok(found.length > 0);
});

test('the IPC gateway may call invoke', () => {
  const found = violations(
    '/repo/src/platform/tauri/ipc.ts',
    "import { invoke } from '@tauri-apps/api/core';\nawait invoke('x');\n",
  );
  assert.deepEqual(found, []);
});

test('a comment that names an API is not an import', () => {
  const found = violations(
    '/repo/src/application/store.mjs',
    "// Calls go through invoke( in platform/tauri, never @tauri-apps here.\n",
  );
  assert.deepEqual(found, []);
});
