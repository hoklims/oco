// OCO Hooks — Shared utilities (cross-platform: Windows, Linux, macOS)
import { createHash } from 'node:crypto';
import { execSync, execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readSync, writeFileSync, appendFileSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { homedir, tmpdir } from 'node:os';

/** Read JSON from stdin (Claude Code pipes hook input). */
export function readStdin() {
  try {
    const chunks = [];
    const fd = 0; // stdin
    const buf = Buffer.alloc(65536);
    let n;
    try { while ((n = readSync(fd, buf)) > 0) chunks.push(buf.slice(0, n)); } catch {}
    return JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } catch { return {}; }
}

/** Async version using process.stdin */
export async function readStdinAsync() {
  return new Promise((resolve) => {
    let data = '';
    let resolved = false;
    const done = () => {
      if (resolved) return;
      resolved = true;
      try { resolve(JSON.parse(data)); } catch { resolve({}); }
    };
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (chunk) => { data += chunk; });
    process.stdin.on('end', done);
    process.stdin.on('error', done);
    // Timeout after 3s in case stdin never closes
    setTimeout(done, 3000);
  });
}

/** Get a stable session state directory, cross-platform. */
export function getStateDir() {
  let workspaceRoot;
  try {
    workspaceRoot = execSync('git rev-parse --show-toplevel', { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }).trim();
  } catch {
    workspaceRoot = process.env.CLAUDE_PROJECT_DIR || process.cwd();
  }

  const hash = createHash('md5').update(workspaceRoot).digest('hex').slice(0, 12);
  const cacheRoot = process.env.XDG_RUNTIME_DIR || join(homedir(), '.cache', 'oco');

  const stateDir = join(cacheRoot, `session-${hash}`);
  try {
    mkdirSync(stateDir, { recursive: true });
    // Basic symlink guard
    if (lstatSync(stateDir).isSymbolicLink()) return join(tmpdir(), 'oco-fallback');
  } catch {}

  return stateDir;
}

/** Check if a command exists on PATH. */
export function commandExists(cmd) {
  try {
    const check = process.platform === 'win32' ? `where ${cmd}` : `command -v ${cmd}`;
    execSync(check, { stdio: ['pipe', 'pipe', 'pipe'] });
    return true;
  } catch { return false; }
}

/** Run OCO CLI command and return parsed JSON. Degrades gracefully. */
export function ocoRun(args) {
  const bin = process.env.OCO_BIN || 'oco';
  if (!commandExists(bin)) return null;
  try {
    const result = execFileSync(bin, args, {
      encoding: 'utf8',
      timeout: 5000,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return JSON.parse(result);
  } catch { return null; }
}

/** Read a state file (returns content or default). */
export function readState(stateDir, filename, defaultVal = '') {
  const p = join(stateDir, filename);
  try { return readFileSync(p, 'utf8').trim(); } catch { return defaultVal; }
}

/** Write a state file. */
export function writeState(stateDir, filename, content) {
  try { writeFileSync(join(stateDir, filename), String(content)); } catch {}
}

/** Append to a state file. */
export function appendState(stateDir, filename, content) {
  try { appendFileSync(join(stateDir, filename), content + '\n'); } catch {}
}

/** Output structured hook response as JSON. */
export function respond(obj) {
  process.stdout.write(JSON.stringify(obj));
}

/** Write error to stderr (shown to Claude when exit 2). */
export function blockWith(message) {
  process.stderr.write(message);
  process.exit(2);
}
