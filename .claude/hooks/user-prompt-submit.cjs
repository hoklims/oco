// OCO Hook: UserPromptSubmit — Task Contract Compiler
// Classifies the task and injects a compact execution contract.
// Persists the contract to state dir for Stop hook enforcement.
'use strict';

const { mkdirSync, writeFileSync, existsSync, lstatSync } = require('fs');
const { join, dirname } = require('path');
const { createHash } = require('crypto');
const { execSync } = require('child_process');
const { homedir, tmpdir } = require('os');

const EMPTY = JSON.stringify({});
const killTimer = setTimeout(() => { process.stdout.write(EMPTY); process.exit(0); }, 3000);
killTimer.unref();
process.on('uncaughtException', () => { process.stdout.write(EMPTY); process.exit(0); });
process.on('unhandledRejection', () => { process.stdout.write(EMPTY); process.exit(0); });

let data = '';
let handled = false;

function done(json) {
  if (handled) return;
  handled = true;
  process.stdout.write(json || EMPTY);
  process.exit(0);
}

// --- State directory (shared with PostToolUse and Stop hooks) ---
function getStateDir() {
  let root;
  try { root = execSync('git rev-parse --show-toplevel', { encoding: 'utf8', timeout: 2000, stdio: ['pipe','pipe','pipe'] }).trim(); } catch { root = process.cwd(); }
  const hash = createHash('md5').update(root).digest('hex').slice(0, 12);
  const base = process.platform === 'win32'
    ? join(process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'), 'oco')
    : (process.env.XDG_RUNTIME_DIR || join(homedir(), '.cache', 'oco'));
  const dir = join(base, `session-${hash}`);
  try { mkdirSync(dir, { recursive: true }); if (lstatSync(dir).isSymbolicLink()) return join(tmpdir(), 'oco-fallback'); } catch {}
  return dir;
}

// --- Classification ---
function classify(prompt) {
  const lower = prompt.toLowerCase();

  // Intent detection
  const bugPatterns = /\b(fix|bug|patch|repair|debug|broken|regression|crash|error|fail|issue)\b/;
  const refactorPatterns = /\b(refactor|rename|restructure|extract|move|reorganize|decouple|split|migrate)\b/;
  const securityPatterns = /\b(security|vulnerability|xss|injection|auth.*bypass|cve)\b/;
  const explainPatterns = /\b(explain|how does|what is|describe|show me|understand|why does)\b/;

  let intent = 'general';
  if (bugPatterns.test(lower)) intent = 'bugfix';
  else if (securityPatterns.test(lower)) intent = 'security';
  else if (refactorPatterns.test(lower)) intent = 'refactor';
  else if (explainPatterns.test(lower)) intent = 'explain';

  // Risk assessment
  const highRiskPatterns = /\b(auth|session|token|password|payment|database|migration|schema|delete|remove|drop|prod)\b/;
  const risk = (intent === 'security' || highRiskPatterns.test(lower)) ? 'high' : 'medium';

  // Verify required?
  const readOnly = (intent === 'explain');
  const verify = !readOnly;

  // Investigation required? (bugfix + non-trivial)
  const investigation = (intent === 'bugfix' || intent === 'security') && risk === 'high';

  // Protocol
  let protocol;
  if (readOnly) protocol = 'read-only';
  else if (investigation) protocol = 'investigate → implement → verify';
  else if (verify) protocol = 'implement → verify';
  else protocol = 'implement';

  // Trivial detection — short prompts with no action keywords
  if (lower.length < 30 && !bugPatterns.test(lower) && !refactorPatterns.test(lower)) {
    return null; // no contract for trivial tasks
  }

  return { intent, risk, verify, investigation, protocol };
}

// --- Main ---
function handle() {
  try {
    const input = data ? JSON.parse(data) : null;
    if (!input || !input.prompt) return done();

    const contract = classify(input.prompt);
    if (!contract) return done(); // trivial — no contract

    // Persist contract to state dir for Stop hook
    const stateDir = getStateDir();
    const state = {
      intent: contract.intent,
      risk: contract.risk,
      verify_required: contract.verify,
      investigation_required: contract.investigation,
      modified_files: [],
      verify_done: false,
      verify_result: null,
      inspection_events_count: 0,
      inspected_before_patch: false,
      stop_blocked_count: 0,
    };

    try {
      const tmpPath = join(stateDir, 'contract.json.tmp');
      const finalPath = join(stateDir, 'contract.json');
      writeFileSync(tmpPath, JSON.stringify(state, null, 2));
      require('fs').renameSync(tmpPath, finalPath);
    } catch {}

    // Build compact contract text for injection
    const lines = [
      `## OCO Contract`,
      `intent: ${contract.intent} | risk: ${contract.risk} | verify: ${contract.verify ? 'required' : 'none'}`,
      `protocol: ${contract.protocol}`,
    ];

    if (contract.verify) {
      lines.push(`blocked_until: build_pass, test_pass`);
    }

    const forbidden = [];
    if (contract.verify) forbidden.push('complete_without_verify');
    if (contract.investigation) forbidden.push('patch_without_investigation');
    if (forbidden.length > 0) {
      lines.push(`forbidden: ${forbidden.join(', ')}`);
    }

    lines.push(`\nThe Stop hook enforces this contract. Completion will be blocked if violated.`);

    done(JSON.stringify({
      hookSpecificOutput: {
        additionalContext: lines.join('\n'),
      },
    }));
  } catch {
    done();
  }
}

process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => { data += chunk; });
process.stdin.on('end', handle);
process.stdin.on('error', () => done());
setTimeout(handle, 500);
