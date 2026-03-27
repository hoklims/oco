// OCO Hook: PostToolUse — Runtime State Tracker
// Maintains contract.json automatically. No model action needed.
// Tracks: modified files, verification status, investigation steps.
import { existsSync, readFileSync, writeFileSync, mkdirSync, lstatSync, renameSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';

const killTimer = setTimeout(() => process.exit(0), 4000);
killTimer.unref();
process.on('uncaughtException', () => process.exit(0));
process.on('unhandledRejection', () => process.exit(0));

// --- State directory ---
function getStateDir() {
  let root;
  try { root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8', timeout: 2000, stdio: ['pipe','pipe','pipe'] }).trim(); } catch { root = process.cwd(); }
  const hash = createHash('md5').update(root).digest('hex').slice(0, 12);
  const base = process.platform === 'win32'
    ? join(process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'), 'oco')
    : (process.env.XDG_RUNTIME_DIR || join(homedir(), '.cache', 'oco'));
  const dir = join(base, `session-${hash}`);
  try { mkdirSync(dir, { recursive: true }); if (lstatSync(dir).isSymbolicLink()) return join(tmpdir(), 'oco-fallback'); } catch {}
  return dir;
}

function loadState(stateDir) {
  const path = join(stateDir, 'contract.json');
  try {
    if (existsSync(path) && !lstatSync(path).isSymbolicLink()) {
      return JSON.parse(readFileSync(path, 'utf8'));
    }
  } catch {}
  return null;
}

function saveState(stateDir, state) {
  try {
    const tmp = join(stateDir, 'contract.json.tmp.' + process.pid);
    const target = join(stateDir, 'contract.json');
    writeFileSync(tmp, JSON.stringify(state, null, 2));
    renameSync(tmp, target);
  } catch {}
}

// --- Verification command detection ---
const VERIFY_PATTERNS = [
  /cargo\s+(test|build|check|clippy|fmt)/,
  /(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?(?:test|build|lint|typecheck|type-check|check)/,
  /(?:npx\s+)?(?:vitest|jest|playwright\s+test|mocha)/,
  /(?:python\s+-m\s+)?pytest/,
  /mypy/, /ruff\s+check/,
  /go\s+(?:test|build|vet)/,
  /dotnet\s+(?:test|build)/,
  /(?:npx\s+)?tsc(?:\s|$)/,
  /make\s+(?:test|check|lint|build)/,
];

// --- Investigation command detection ---
const INVESTIGATION_PATTERNS = [
  /\b(grep|rg|find|oco\s+search|git\s+log|git\s+blame|git\s+diff|git\s+show|cat\s+|head\s+|tail\s+)\b/,
];

function matchesAny(cmd, patterns) {
  const lower = (cmd || '').toLowerCase();
  return patterns.some(p => p.test(lower));
}

// --- Stdin reader ---
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
  const state = loadState(stateDir);

  // No contract active — nothing to track
  if (!state) process.exit(0);

  let changed = false;

  // --- Track modified files ---
  if (['Edit', 'Write', 'MultiEdit'].includes(toolName) && !toolError) {
    const filePath = input.tool_input?.file_path || input.tool_input?.path || input.tool_input?.destination || '';
    if (filePath) {
      if (!state.modified_files) state.modified_files = [];
      if (!state.modified_files.includes(filePath)) {
        state.modified_files.push(filePath);
      }
      // Any edit invalidates previous verification
      state.verify_done = false;
      state.verify_result = 'unknown';
      changed = true;
    }
  }

  // --- Bash commands: verification + investigation ---
  if ((toolName === 'Bash' || toolName === 'bash')) {
    const command = input.tool_input?.command || '';

    if (matchesAny(command, VERIFY_PATTERNS)) {
      state.verify_done = true;
      state.verify_result = toolError ? 'fail' : 'pass';
      changed = true;
    } else if (!toolError && matchesAny(command, INVESTIGATION_PATTERNS)) {
      state.inspection_events_count = (state.inspection_events_count || 0) + 1;
      // Mark inspected_before_patch if no files modified yet
      if (!state.modified_files || state.modified_files.length === 0) {
        state.inspected_before_patch = true;
      }
      changed = true;
    }
  }

  if (changed) saveState(stateDir, state);
} catch {}

process.exit(0);
