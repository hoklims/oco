// OCO Hook: UserPromptSubmit (cross-platform)
// Lightweight inline triage — NO subprocess, pure heuristics.
// Must complete in <500ms.

const killTimer = setTimeout(() => process.exit(0), 3000);
killTimer.unref();
process.on('uncaughtException', () => process.exit(0));
process.on('unhandledRejection', () => process.exit(0));

try {
  // --- Read stdin (async, 1s timeout) ---
  const input = await new Promise((resolve) => {
    let data = '', done = false;
    const finish = () => { if (done) return; done = true; try { resolve(JSON.parse(data)); } catch { resolve(null); } };
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (c) => { data += c; });
    process.stdin.on('end', finish);
    process.stdin.on('error', finish);
    setTimeout(finish, 1000);
  });

  if (!input?.prompt) process.exit(0);

  const prompt = input.prompt.toLowerCase();

  // --- Inline classification (zero subprocess, pure regex) ---
  const highPatterns = /\b(refactor|rewrite|migrate|redesign|overhaul|rearchitect|restructure)\b/;
  const criticalPatterns = /\b(delete.*prod|drop.*database|reset.*hard|force.*push|rm\s+-rf)\b/;
  const verifyPatterns = /\b(fix|bug|patch|repair|resolve|debug|test|build|deploy)\b/;

  let complexity = 'medium';
  if (criticalPatterns.test(prompt)) complexity = 'critical';
  else if (highPatterns.test(prompt)) complexity = 'high';
  else if (prompt.length < 30 && !verifyPatterns.test(prompt)) complexity = 'trivial';

  const needsVerify = verifyPatterns.test(prompt) || complexity === 'high' || complexity === 'critical';

  if (complexity === 'trivial') process.exit(0);

  let guidance = `[OCO] complexity=${complexity} verify=${needsVerify}`;

  if (complexity === 'high' || complexity === 'critical') {
    guidance += ' | Recommended: investigate before acting.';
  } else if (needsVerify) {
    guidance += ' | Verify after changes.';
  }

  process.stdout.write(JSON.stringify({ hookSpecificOutput: { additionalContext: guidance } }));
} catch {}

process.exit(0);
