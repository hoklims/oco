// OCO Hook: Stop (cross-platform)
// Prevents premature completion when code was modified without verification.
// MUST exit within 4s no matter what.
// MUST always write JSON to stdout — empty stdout = "hook error" in Claude Code.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, unlinkSync, mkdirSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';

const EMPTY = '{}';

let _exiting = false;
function safeExit(code = 0, json = EMPTY) {
  if (_exiting) return;
  _exiting = true;
  try {
    if (code === 2) {
      // Blocking exit: stderr already written, just exit
      process.exit(2);
    }
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
  const stopReason = input?.reason || 'complete';

  // Only enforce verification for completion stops
  if (stopReason !== 'complete' && stopReason !== '') safeExit(0);

  const stateDir = getStateDir();

  // Check if files were modified during this session
  const modifiedLog = join(stateDir, 'modified-files');
  if (!existsSync(modifiedLog)) safeExit(0);

  let modifiedFiles;
  try {
    modifiedFiles = [...new Set(readFileSync(modifiedLog, 'utf8').split('\n').filter(Boolean))];
  } catch { safeExit(0); }

  if (modifiedFiles.length === 0) safeExit(0);

  // Filter to only files under the current project's git root
  let gitRoot;
  try { gitRoot = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8', timeout: 2000, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }).trim().replace(/\\/g, '/'); } catch { gitRoot = null; }
  if (gitRoot) {
    modifiedFiles = modifiedFiles.filter(f => f.replace(/\\/g, '/').startsWith(gitRoot));
    if (modifiedFiles.length === 0) safeExit(0);
  }

  // Ignore non-source files (hooks, configs, docs)
  const nonSourcePatterns = [/[/\\]\.claude[/\\]/, /[/\\]\.github[/\\]/, /[/\\]docs[/\\]/, /\.md$/i, /\.json$/i, /\.ya?ml$/i, /\.toml$/i, /\.mjs$/i, /\.cjs$/i, /\.sh$/i, /\.bash$/i, /\.zsh$/i, /Makefile$/i, /Dockerfile$/i, /\.dockerignore$/i, /\.env/i, /\.gitignore$/i, /\.editorconfig$/i, /\.prettierrc/i, /\.eslintrc/i, /\.lock$/i];
  const sourceFiles = modifiedFiles.filter(f => !nonSourcePatterns.some(p => p.test(f)));
  if (sourceFiles.length === 0) safeExit(0);

  // Check verification timestamp vs last modification
  const verifyTs = parseInt(readState(stateDir, 'verify-done', '0'), 10);
  const modifiedTs = parseInt(readState(stateDir, 'last-modified-ts', '0'), 10);

  if (verifyTs >= modifiedTs && verifyTs > 0) {
    // Verification happened after last modification — clean and allow
    try {
      unlinkSync(modifiedLog);
      unlinkSync(join(stateDir, 'verify-done'));
      unlinkSync(join(stateDir, 'last-modified-ts'));
    } catch {}
    safeExit(0);
  }

  // Determine needed checks from project manifests
  const cwd = input.cwd || process.env.CLAUDE_PROJECT_DIR || process.cwd();
  const checks = [];
  if (existsSync(join(cwd, 'Cargo.toml'))) checks.push('build', 'test', 'clippy');
  if (existsSync(join(cwd, 'package.json'))) {
    try {
      const pkg = JSON.parse(readFileSync(join(cwd, 'package.json'), 'utf8'));
      if (pkg.scripts?.test) checks.push('test');
      if (pkg.scripts?.lint) checks.push('lint');
    } catch { checks.push('test'); }
  }
  if (existsSync(join(cwd, 'pyproject.toml')) || existsSync(join(cwd, 'setup.py'))) checks.push('test', 'typecheck');

  if (checks.length === 0) safeExit(0);

  const fileList = sourceFiles.slice(0, 10).join(',');
  process.stderr.write(`OCO: ${sourceFiles.length} file(s) modified [${fileList}] but no verification run detected. Recommended checks: ${checks.join(',')}. Run build/test/lint before completing.`);
  safeExit(2);
} catch {}

safeExit(0);
