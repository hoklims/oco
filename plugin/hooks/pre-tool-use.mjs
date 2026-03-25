// OCO Hook: PreToolUse (cross-platform)
// Enforces tool policy gates before execution.
// MUST exit within 4s no matter what.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, mkdirSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';

const killTimer = setTimeout(() => process.exit(0), 4000);
killTimer.unref();
process.on('uncaughtException', () => process.exit(0));
process.on('unhandledRejection', () => process.exit(0));

// --- Helpers (inlined, no external deps) ---
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
function respond(obj) { process.stdout.write(JSON.stringify(obj)); }
function blockWith(msg) { process.stderr.write(msg); process.exit(2); }

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
  const toolInput = input?.tool_input || {};
  if (!toolName) process.exit(0);

  const stateDir = getStateDir();

  // --- Destructive command detection ---
  if (toolName === 'Bash' || toolName === 'bash') {
    const command = (toolInput.command || '').toLowerCase();
    const destructive = [
      'rm -rf', 'rm -r ', 'rmdir',
      'git reset --hard', 'git push --force', 'git push -f ',
      'git clean -fd', 'git checkout -- .', 'git restore .',
      'drop table', 'drop database', 'truncate table',
    ];
    for (const pattern of destructive) {
      if (command.includes(pattern)) {
        blockWith(`OCO policy: destructive command detected (${pattern}). Use a safer alternative or confirm explicitly.`);
      }
    }
  }

  // --- Sensitive file protection ---
  if (['Edit', 'Write', 'MultiEdit'].includes(toolName)) {
    const filePath = (toolInput.file_path || toolInput.path || '').toLowerCase();
    const sensitive = ['.env', 'credentials', 'secrets', '.key', '.pem', 'id_rsa'];
    for (const pattern of sensitive) {
      if (filePath.includes(pattern)) {
        blockWith(`OCO policy: write to sensitive file (${pattern}) blocked. Review manually.`);
      }
    }
  }

  // --- Loop detection ---
  const loopFile = `loop-${toolName.replace(/[^a-zA-Z0-9]/g, '_')}`;
  let count = parseInt(readState(stateDir, loopFile, '0'), 10) + 1;
  writeState(stateDir, loopFile, String(count));

  if (count >= 5) {
    if (count >= 8) writeState(stateDir, loopFile, '0');
    respond({ hookSpecificOutput: { additionalContext: `OCO: tool '${toolName}' called ${count} times. Possible loop — consider a different approach.` } });
    process.exit(0);
  }

  // --- OCO advanced gate check (optional, fire-and-forget on failure) ---
  try {
    const bin = process.env.OCO_BIN || 'oco';
    const raw = execFileSync(bin, ['gate-check', '--tool', toolName, '--input', JSON.stringify(toolInput), '--format', 'json'], {
      encoding: 'utf8', timeout: 2000, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true,
    });
    const gateResult = JSON.parse(raw);
    if (gateResult?.decision === 'deny') {
      blockWith(`OCO policy: ${gateResult.reason || 'denied by policy'}`);
    }
  } catch {}
} catch {}

process.exit(0);
