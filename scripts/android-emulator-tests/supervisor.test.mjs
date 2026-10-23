import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
const supervisor = fileURLToPath(new URL('./emulator-supervise.sh', import.meta.url));
for (const [label, command, expected] of [
  ['normal exit', 'exit 0', 0],
  ['failure exit', 'exit 17', 17],
  ['signal termination', 'kill -TERM $$', 143],
]) {
  test(`supervisor records ${label}`, async () => {
    const dir = await mkdtemp(path.join(tmpdir(), 'pm-supervisor-'));
    try {
      const statusFile = path.join(dir, 'exit.txt');
      const pidFile = path.join(dir, 'pid');
      const child = spawn('bash', [supervisor, statusFile, pidFile, 'bash', '-c', command], { stdio: 'ignore' });
      const [code] = await once(child, 'exit');
      assert.equal(code, expected);
      assert.match(await readFile(statusFile, 'utf8'), new RegExp(`^status=${expected} time=\\d{4}-`));
      assert.match(await readFile(pidFile, 'utf8'), /^\d+\n$/);
    } finally { await rm(dir, { recursive: true, force: true }); }
  });
}
