#!/usr/bin/env node
/**
 * E2E tests for oco-claude-plugin CLI (install, doctor, repair, uninstall).
 *
 * Runs against a temporary directory to avoid polluting the real project.
 * Requires: Node >= 18, git on PATH.
 *
 * Usage: node tests/plugin-cli.test.mjs
 */

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync, readFileSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const CLI = join(__dirname, '..', 'cli.mjs');
const WORK = join(tmpdir(), `oco-plugin-test-${Date.now()}`);

let passed = 0;
let failed = 0;

function assert(condition, msg) {
  if (condition) {
    console.log(`  ✓ ${msg}`);
    passed++;
  } else {
    console.log(`  ✗ ${msg}`);
    failed++;
  }
}

function run(args, opts = {}) {
  try {
    const result = execFileSync('node', [CLI, ...args], {
      cwd: opts.cwd || WORK,
      encoding: 'utf8',
      timeout: 15000,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ...opts.env },
    });
    return { stdout: result, exitCode: 0 };
  } catch (err) {
    return { stdout: err.stdout || '', stderr: err.stderr || '', exitCode: err.status };
  }
}

// --- Setup ---
console.log(`\nOCO Plugin CLI — E2E Tests\n`);
console.log(`Work dir: ${WORK}\n`);

mkdirSync(WORK, { recursive: true });
execFileSync('git', ['init'], { cwd: WORK, stdio: 'pipe' });
execFileSync('git', ['config', 'user.email', 'test@test.com'], { cwd: WORK, stdio: 'pipe' });
execFileSync('git', ['config', 'user.name', 'Test'], { cwd: WORK, stdio: 'pipe' });

// Create a minimal package.json so findProjectRoot works
const pkgPath = join(WORK, 'package.json');
const fs = await import('node:fs');
fs.writeFileSync(pkgPath, '{}');

// --- Test: install ---
console.log('Install');
{
  const r = run(['install']);
  assert(r.exitCode === 0, 'install exits 0');
  assert(r.stdout.includes('OCO Claude Code Plugin'), 'install shows header');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'pre-tool-use.mjs')), 'pre-tool-use.mjs created');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'post-tool-use.mjs')), 'post-tool-use.mjs created');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'stop.mjs')), 'stop.mjs created');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'user-prompt-submit.cjs')), 'user-prompt-submit.cjs created');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'lib', 'utils.mjs')), 'utils.mjs created');
  assert(existsSync(join(WORK, '.claude', 'mcp', 'bridge.cjs')), 'bridge.cjs created');
  assert(existsSync(join(WORK, '.claude', 'skills', 'oco-verify-fix', 'SKILL.md')), 'skill verify-fix created');
  assert(existsSync(join(WORK, '.claude', 'agents', 'patch-verifier.md')), 'agent patch-verifier created');
  assert(existsSync(join(WORK, '.claude', '.oco-install-manifest.json')), 'manifest created');

  // Check manifest content
  const manifest = JSON.parse(readFileSync(join(WORK, '.claude', '.oco-install-manifest.json'), 'utf8'));
  assert(manifest.version === JSON.parse(readFileSync(join(__dirname, '..', 'package.json'), 'utf8')).version, 'manifest version matches package.json');
  assert(manifest.files.length >= 14, `manifest lists ${manifest.files.length} files (>= 14)`);

  // Post-install diagnostic
  assert(r.stdout.includes('Post-install check'), 'install shows post-install diagnostic');
  assert(r.stdout.includes('Mode:'), 'install shows mode');
}

// --- Test: doctor ---
console.log('\nDoctor (healthy)');
{
  const r = run(['doctor']);
  // exit code may be 0 or 1 depending on global install detection
  assert(r.exitCode === 0 || r.exitCode === 1, `doctor exits ${r.exitCode} (0 or 1)`);
  assert(r.stdout.includes('4/4 hooks'), 'doctor finds all hooks');
  assert(r.stdout.includes('5/5 skills'), 'doctor finds all skills');
  assert(r.stdout.includes('3/3 agents'), 'doctor finds all agents');
  assert(r.stdout.includes('MCP bridge'), 'doctor checks bridge');
  assert(r.stdout.includes('Mode:'), 'doctor shows mode');
}

// --- Test: break then doctor ---
console.log('\nDoctor (broken)');
{
  unlinkSync(join(WORK, '.claude', 'hooks', 'stop.mjs'));
  unlinkSync(join(WORK, '.claude', 'hooks', 'pre-tool-use.mjs'));

  const r = run(['doctor']);
  assert(r.exitCode === 2, 'doctor exits 2 when broken');
  assert(r.stdout.includes('2/4 hooks'), 'doctor detects missing hooks');
  assert(r.stdout.includes('broken'), 'doctor reports broken mode');
}

// --- Test: repair --dry-run ---
console.log('\nRepair (dry-run)');
{
  const r = run(['repair', '--dry-run']);
  assert(r.stdout.includes('would restore'), 'dry-run shows what would be restored');
  assert(!existsSync(join(WORK, '.claude', 'hooks', 'stop.mjs')), 'dry-run does not restore files');
}

// --- Test: repair ---
console.log('\nRepair');
{
  const r = run(['repair']);
  assert(r.exitCode === 0, 'repair exits 0');
  assert(r.stdout.includes('restored'), 'repair restores files');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'stop.mjs')), 'stop.mjs restored');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'pre-tool-use.mjs')), 'pre-tool-use.mjs restored');
}

// --- Test: doctor after repair ---
console.log('\nDoctor (after repair)');
{
  const r = run(['doctor']);
  assert(r.exitCode === 0 || r.exitCode === 1, `doctor exits ${r.exitCode} after repair`);
  assert(r.stdout.includes('4/4 hooks'), 'all hooks restored');
  assert(!r.stdout.includes('broken'), 'no longer broken');
}

// --- Test: uninstall ---
console.log('\nUninstall');
{
  const r = run(['uninstall']);
  assert(r.exitCode === 0, 'uninstall exits 0');
  assert(!existsSync(join(WORK, '.claude', '.oco-install-manifest.json')), 'manifest removed');
  assert(!existsSync(join(WORK, '.claude', 'hooks', 'pre-tool-use.mjs')), 'hooks removed');
}

// --- Test: doctor after uninstall ---
console.log('\nDoctor (after uninstall)');
{
  const r = run(['doctor']);
  // Should detect not installed (project) — may fallback to global
  assert(r.stdout.includes('Not installed') || r.stdout.includes('Source: global'),
    'doctor detects uninstall or falls back to global');
}

// --- Test: install --force (fresh from uninstall) ---
console.log('\nInstall --force (fresh)');
{
  const r = run(['install', '--force']);
  assert(r.exitCode === 0, 'install --force exits 0');
  assert(existsSync(join(WORK, '.claude', 'hooks', 'pre-tool-use.mjs')), 'hooks created by force install');
  assert(existsSync(join(WORK, '.claude', '.oco-install-manifest.json')), 'manifest created by force install');
}

// --- Cleanup ---
rmSync(WORK, { recursive: true, force: true });

// --- Results ---
console.log(`\n${'='.repeat(40)}`);
console.log(`Results: ${passed} passed, ${failed} failed`);
console.log(`${'='.repeat(40)}\n`);

process.exit(failed > 0 ? 1 : 0);
