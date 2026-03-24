// OCO Hook: PostToolUse (cross-platform)
// Records observations, tracks modifications and verification timestamps.
// MUST exit within 4s no matter what.
import { execFile } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, appendFileSync, mkdirSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';

const killTimer = setTimeout(() => process.exit(0), 4000);
killTimer.unref();
process.on('uncaughtException', () => process.exit(0));
process.on('unhandledRejection', () => process.exit(0));

// --- Helpers (inlined) ---
function getStateDir() {
  let root;
  try { root = require('node:child_process').execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8', timeout: 2000, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }).trim(); } catch { root = process.env.CLAUDE_PROJECT_DIR || process.cwd(); }
  const hash = createHash('md5').update(root).digest('hex').slice(0, 12);
  const dir = join(process.env.XDG_RUNTIME_DIR || join(homedir(), '.cache', 'oco'), `session-${hash}`);
  try { mkdirSync(dir, { recursive: true }); if (lstatSync(dir).isSymbolicLink()) return join(tmpdir(), 'oco-fallback'); } catch {}
  return dir;
}
function readState(dir, file, def = '') { try { return readFileSync(join(dir, file), 'utf8').trim(); } catch { return def; } }
function writeState(dir, file, val) { try { writeFileSync(join(dir, file), String(val)); } catch {} }
function appendState(dir, file, val) { try { appendFileSync(join(dir, file), val + '\n'); } catch {} }

function readStdin() {
  return new Promise((resolve) => {
    let data = '', done = false;
    const finish = () => { if (done) return; done = true; try { resolve(JSON.parse(data)); } catch { resolve(null); } };
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (c) => { data += c; });
    process.stdin.on('end', finish);
    process.stdin.on('error', finish);
    setTimeout(finish, 1000);
  });
}

try {
  const input = await readStdin();
  const toolName = input?.tool_name || '';
  const toolError = input?.error || '';
  if (!toolName) process.exit(0);

  const stateDir = getStateDir();
  const now = Math.floor(Date.now() / 1000);

  // --- Telemetry: record observation (async, fire-and-forget) ---
  try {
    const bin = process.env.OCO_BIN || 'oco';
    execFile(bin, ['observe', '--tool', toolName, '--status', toolError ? 'error' : 'ok', '--format', 'json'], { timeout: 3000, windowsHide: true });
  } catch {}

  // --- Track modified files ---
  if (['Edit', 'Write', 'MultiEdit'].includes(toolName)) {
    const filePath = input.tool_input?.file_path || input.tool_input?.path || input.tool_input?.destination || '';
    if (filePath) {
      appendState(stateDir, 'modified-files', filePath);
      writeState(stateDir, 'last-modified-ts', String(now));
    }
  }

  // --- Detect verification commands ---
  if (!toolError && (toolName === 'Bash' || toolName === 'bash')) {
    const command = (input.tool_input?.command || '').toLowerCase();
    const verifyCmds = [
      'cargo test', 'cargo build', 'cargo check', 'cargo clippy',
      'npm test', 'npm run build', 'npm run lint',
      'pytest', 'python -m pytest', 'go test', 'go build',
      'tsc --noemit', 'npx tsc', 'mypy', 'ruff check',
    ];
    for (const vc of verifyCmds) {
      if (command.startsWith(vc) || command.includes(` && ${vc}`) || command.includes(`; ${vc}`)) {
        writeState(stateDir, 'verify-done', String(now));
        break;
      }
    }
  }

  // --- Reset loop counter on success ---
  if (!toolError) {
    const loopFile = `loop-${toolName.replace(/[^a-zA-Z0-9]/g, '_')}`;
    writeState(stateDir, loopFile, '0');
  }
} catch {}

process.exit(0);
