// OCO Hook: PostToolUse (cross-platform)
// Records observations, tracks modifications and verification timestamps.
// MUST exit within 4s no matter what.
// MUST always write JSON to stdout — empty stdout = "hook error" in Claude Code.
import { execFile, execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, appendFileSync, mkdirSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';

const EMPTY = '{}';

let _exiting = false;
function safeExit(code = 0, json = EMPTY) {
  if (_exiting) return;
  _exiting = true;
  try {
    process.stdout.write(json, () => process.exit(code));
  } catch {
    process.exit(code);
  }
  setTimeout(() => process.exit(code), 500).unref();
}

const killTimer = setTimeout(() => safeExit(0), 4000);
killTimer.unref();
process.on('uncaughtException', () => safeExit(0));
process.on('unhandledRejection', () => safeExit(0));

// --- Helpers (inlined) ---
function getStateDir() {
  let root;
  try { root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8', timeout: 2000, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }).trim(); } catch { root = process.env.CLAUDE_PROJECT_DIR || process.cwd(); }
  const hash = createHash('md5').update(root).digest('hex').slice(0, 12);
  const cacheRoot = process.env.XDG_RUNTIME_DIR
    || (process.platform === 'win32'
      ? join(process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'), 'oco')
      : join(homedir(), '.cache', 'oco'));
  const dir = join(cacheRoot, `session-${hash}`);
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
  if (!toolName) safeExit(0);

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
    const verifyPatterns = [
      // Rust
      /cargo\s+(test|build|check|clippy|fmt)/,
      // JS/TS package managers
      /(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?(?:test|build|lint|typecheck|type-check|check)/,
      /(?:npm|pnpm|yarn|bun)\s+(?:--filter\s+\S+\s+)?(?:test|build|lint|typecheck|type-check)/,
      // Direct test runners
      /(?:npx\s+)?(?:vitest|jest|playwright\s+test|mocha|ava)/,
      // Python
      /(?:python\s+-m\s+)?pytest/,
      /mypy/, /ruff\s+check/,
      // Go
      /go\s+(?:test|build|vet)/,
      // .NET
      /dotnet\s+(?:test|build)/,
      // TypeScript
      /(?:npx\s+)?tsc(?:\s|$)/,
      // Make
      /make\s+(?:test|check|lint|build)/,
    ];
    for (const pat of verifyPatterns) {
      if (pat.test(command)) {
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

safeExit(0);
