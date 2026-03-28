// OCO Hook: UserPromptSubmit (CommonJS, callback-based, no deadlock)
// Non-blocking stdin — avoids readSync deadlock on Windows.
// ALWAYS writes JSON to stdout — empty stdout triggers false "hook error" in Claude Code.
// See: https://github.com/anthropics/claude-code/issues/36948
'use strict';

const EMPTY = JSON.stringify({});

// Safe exit: flush stdout before killing the process.
// On Windows, process.stdout.write() to a pipe is async — process.exit()
// can kill the process before the write buffer is flushed.
function safeExit(code, json) {
  try {
    process.stdout.write(json || EMPTY, () => process.exit(code || 0));
  } catch {
    process.exit(code || 0);
  }
  // Fallback: if callback never fires (broken pipe), force exit after 500ms
  setTimeout(() => process.exit(code || 0), 500).unref();
}

const killTimer = setTimeout(() => safeExit(0), 3000);
killTimer.unref();
process.on('uncaughtException', () => safeExit(0));
process.on('unhandledRejection', () => safeExit(0));

let data = '';
let handled = false;

function done(json) {
  if (handled) return;
  handled = true;
  safeExit(0, json || EMPTY);
}

function handle() {
  try {
    const input = data ? JSON.parse(data) : null;
    if (!input || !input.prompt) return done();

    const prompt = input.prompt.toLowerCase();

    const highPatterns = /\b(refactor|rewrite|migrate|redesign|overhaul|rearchitect|restructure)\b/;
    const criticalPatterns = /\b(delete.*prod|drop.*database|reset.*hard|force.*push|rm\s+-rf)\b/;
    const verifyPatterns = /\b(fix|bug|patch|repair|resolve|debug|test|build|deploy)\b/;

    let complexity = 'medium';
    if (criticalPatterns.test(prompt)) complexity = 'critical';
    else if (highPatterns.test(prompt)) complexity = 'high';
    else if (prompt.length < 30 && !verifyPatterns.test(prompt)) complexity = 'trivial';

    if (complexity === 'trivial') return done();

    const needsVerify = verifyPatterns.test(prompt) || complexity === 'high' || complexity === 'critical';

    let guidance = '[OCO] complexity=' + complexity + ' verify=' + needsVerify;
    if (complexity === 'high' || complexity === 'critical') {
      guidance += ' | Recommended: investigate before acting.';
    } else if (needsVerify) {
      guidance += ' | Verify after changes.';
    }

    done(JSON.stringify({ hookSpecificOutput: { additionalContext: guidance } }));
  } catch (e) {
    done();
  }
}

process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => { data += chunk; });
process.stdin.on('end', handle);
process.stdin.on('error', () => done());
setTimeout(handle, 500);
