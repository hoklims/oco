// OCO Hook: Stop — Contract Enforcer
// Blocks completion if the task contract is not satisfied.
// Rules:
//   1. modified_files > 0 AND !verify_done → BLOCK
//   2. investigation_required AND investigation_steps === 0 AND modified_files > 0 → BLOCK
//   3. stop_blocked_count >= 3 → WARN + ALLOW (anti-loop)
import { existsSync, readFileSync, writeFileSync, lstatSync, renameSync, unlinkSync, mkdirSync } from 'node:fs';
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

function cleanState(stateDir) {
  try { unlinkSync(join(stateDir, 'contract.json')); } catch {}
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
  const reason = input?.reason || 'complete';

  // Only enforce on completion stops
  if (reason !== 'complete' && reason !== '') process.exit(0);

  const stateDir = getStateDir();
  const state = loadState(stateDir);

  // No contract → nothing to enforce
  if (!state) process.exit(0);

  const modified = (state.modified_files || []).length;
  const violations = [];

  // --- Rule 1: verification required ---
  if (modified > 0 && state.verify_required && !state.verify_done) {
    violations.push(`${modified} file(s) modified but not verified. Run build/test/lint.`);
  }

  // --- Rule 2: investigation required ---
  if (state.investigation_required && !state.inspected_before_patch && modified > 0) {
    violations.push('High-risk task requires inspection before patching. Search/read relevant code first.');
  }

  // --- Rule 3: anti-loop override ---
  if (violations.length > 0) {
    state.stop_blocked_count = (state.stop_blocked_count || 0) + 1;
    saveState(stateDir, state);

    if (state.stop_blocked_count >= 3) {
      // Override: allow after 3 blocks to prevent infinite loop
      state.override_reason = 'max_stop_blocks_reached';
      saveState(stateDir, state);
      process.stderr.write(
        `OCO: override (${state.override_reason}). Remaining violations:\n` +
        violations.map(v => `  - ${v}`).join('\n') + '\n'
      );
      cleanState(stateDir);
      process.exit(0); // allow
    }

    // Block completion
    process.stderr.write(
      `OCO contract (${state.stop_blocked_count}/3 before override):\n` +
      violations.map(v => `  - ${v}`).join('\n') + '\n'
    );
    process.exit(2);
  }

  // All good — clean up state and allow
  cleanState(stateDir);
  process.exit(0);
} catch {}

process.exit(0);
