// OCO Hook: Stop (cross-platform)
// Prevents premature completion when code was modified without verification.
// MUST exit within 4s no matter what.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, unlinkSync, mkdirSync, lstatSync } from 'node:fs';
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
  try { root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8', timeout: 2000, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }).trim(); } catch { root = process.env.CLAUDE_PROJECT_DIR || process.cwd(); }
  const hash = createHash('md5').update(root).digest('hex').slice(0, 12);
  const dir = join(process.env.XDG_RUNTIME_DIR || join(homedir(), '.cache', 'oco'), `session-${hash}`);
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
  if (stopReason !== 'complete' && stopReason !== '') process.exit(0);

  const stateDir = getStateDir();

  // Check if files were modified during this session
  const modifiedLog = join(stateDir, 'modified-files');
  if (!existsSync(modifiedLog)) process.exit(0);

  let modifiedFiles;
  try {
    modifiedFiles = [...new Set(readFileSync(modifiedLog, 'utf8').split('\n').filter(Boolean))];
  } catch { process.exit(0); }

  if (modifiedFiles.length === 0) process.exit(0);

  // Ignore non-source files (hooks, configs, docs) — no verification needed
  // Paths may be absolute; match against full path patterns
  const nonSourcePatterns = [/[/\\]\.claude[/\\]/, /[/\\]\.github[/\\]/, /[/\\]docs[/\\]/, /\.md$/i, /\.json$/i, /\.ya?ml$/i, /\.toml$/i, /\.mjs$/i, /\.cjs$/i];
  const sourceFiles = modifiedFiles.filter(f => !nonSourcePatterns.some(p => p.test(f)));
  if (sourceFiles.length === 0) process.exit(0);

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
    process.exit(0);
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

  if (checks.length === 0) process.exit(0);

  const fileList = sourceFiles.slice(0, 10).join(',');
  process.stderr.write(`OCO: ${sourceFiles.length} file(s) modified [${fileList}] but no verification run detected. Recommended checks: ${checks.join(',')}. Run build/test/lint before completing.`);
  process.exit(2);
} catch {}

process.exit(0);
