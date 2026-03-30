#!/usr/bin/env node
/**
 * OCO Claude Code Plugin — Installer / Uninstaller
 *
 * Usage:
 *   npx oco-claude-plugin install [--global] [--force]
 *   npx oco-claude-plugin uninstall [--global]
 *   npx oco-claude-plugin status [--global]
 *
 * Zero external dependencies. Node >= 18.
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync, readdirSync, statSync, renameSync } from 'node:fs';
import { join, dirname, relative, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { execSync, spawnSync } from 'node:child_process';

/** Parse JSON file safely — returns fallback on read/parse error. */
function safeReadJson(path, fallback = {}) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return fallback;
  }
}

/** Write via temp file + rename to prevent corruption on crash. */
function atomicWriteFileSync(path, content) {
  const tmp = path + '.tmp';
  writeFileSync(tmp, content);
  try {
    renameSync(tmp, path);
  } catch {
    // Cross-device fallback (EXDEV)
    try { writeFileSync(path, readFileSync(tmp)); } catch { /* propagate original */ }
    try { rmSync(tmp); } catch { /* ignore cleanup failure */ }
  }
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const PLUGIN_SRC = join(__dirname, 'plugin');
const MANIFEST_FILE = '.oco-install-manifest.json';
const DROPIN_FILE = 'managed-settings.d/50-oco.json';
const VERSION = JSON.parse(readFileSync(join(__dirname, 'package.json'), 'utf8')).version;

const MODE_DESCRIPTIONS = {
  full: 'plugin + runtime — all features active',
  'plugin-only': 'plugin installed, runtime not found — hooks, skills, agents work; MCP tools return fallback results',
  incomplete: 'some plugin files missing — run: npx oco-claude-plugin repair',
  broken: 'settings or hooks missing — plugin will not load, run: npx oco-claude-plugin install --force',
};

function resolveMode({ settingsOk, allHooksOk, bridgeOk, ocoAvailable }) {
  if (!settingsOk || !allHooksOk) return 'broken';
  if (!bridgeOk) return 'incomplete';
  if (ocoAvailable) return 'full';
  return 'plugin-only';
}

function getOcoVersion() {
  try {
    const res = spawnSync('oco', ['--version'], {
      encoding: 'utf8', timeout: 3000, windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const match = (res.stdout || '').trim().match(/\b(\d+\.\d+\.\d+)/);
    return match ? match[1] : null;
  } catch { return null; }
}

function checkOcoUsable() {
  try {
    const res = spawnSync('oco', ['--help'], {
      encoding: 'utf8', timeout: 5000, windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return res.status === 0;
  } catch { return false; }
}

/**
 * Run `claude mcp list` and check if oco appears.
 * Returns: 'connected' | 'listed' | 'missing' | 'unavailable'
 */
function checkMcpHealth() {
  try {
    const res = spawnSync('claude', ['mcp', 'list'], {
      encoding: 'utf8', timeout: 30000, windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const output = (res.stdout || '') + (res.stderr || '');
    if (!output.includes('mcp') && !output.includes('MCP') && !output.includes('Connected')) {
      return 'unavailable';
    }
    // Look for oco line in output
    const lines = output.split('\n');
    const ocoLine = lines.find(l => /^oco[:\s]/i.test(l.trim()));
    if (!ocoLine) return 'missing';
    if (ocoLine.includes('Connected')) return 'connected';
    return 'listed';
  } catch { return 'unavailable'; }
}

// --- CLI Argument Parsing ---

const [,, command, ...args] = process.argv;
const isGlobal = args.includes('--global') || args.includes('-g');
const isForce = args.includes('--force') || args.includes('-f');
const isDryRun = args.includes('--dry-run');

switch (command) {
  case 'install':   await install(); break;
  case 'uninstall': await uninstall(); break;
  case 'status':    status(); break;
  case 'doctor':    process.exit(doctor()); break;
  case 'repair':    process.exit(await repair()); break;
  default:          usage(); break;
}

// --- Install ---

async function install() {
  const targetDir = resolveTarget();
  const settingsPath = join(targetDir, 'settings.json');

  console.log(`\n  OCO Claude Code Plugin v${VERSION}`);
  console.log(`  Target: ${targetDir}\n`);

  // 1. Collect files to copy
  const files = collectFiles(PLUGIN_SRC);
  const pluginFiles = files.filter(f => f !== 'settings-fragment.json');

  // 2. Copy plugin files
  let copied = 0;
  let skipped = 0;

  for (const relPath of pluginFiles) {
    const src = join(PLUGIN_SRC, relPath);
    const dest = join(targetDir, relPath);
    const destDir = dirname(dest);

    if (!existsSync(destDir)) {
      mkdirSync(destDir, { recursive: true });
    }

    if (existsSync(dest) && !isForce) {
      skipped++;
      console.log(`  skip  ${relPath} (exists, use --force to overwrite)`);
      continue;
    }

    try {
      // Use read+write instead of cpSync to avoid Windows NTFS false errors
      writeFileSync(dest, readFileSync(src));
      copied++;
      console.log(`  copy  ${relPath}`);
    } catch (err) {
      skipped++;
      console.log(`  skip  ${relPath} (${err.code || 'copy failed'})`);
    }
  }

  // 3. Write settings — prefer managed-settings.d/ drop-in (Claude Code v2.1.83+)
  const fragment = JSON.parse(readFileSync(join(PLUGIN_SRC, 'settings-fragment.json'), 'utf8'));

  // Adjust paths for global install
  if (isGlobal) {
    rewritePathsForGlobal(fragment, targetDir);
  }

  const dropinDir = join(targetDir, 'managed-settings.d');
  const dropinPath = join(targetDir, DROPIN_FILE);
  const useDropin = supportsDropin(targetDir);

  if (useDropin) {
    // managed-settings.d/ drop-in — isolated, no merge conflicts
    if (!existsSync(dropinDir)) {
      mkdirSync(dropinDir, { recursive: true });
    }
    atomicWriteFileSync(dropinPath, JSON.stringify(fragment, null, 2) + '\n');
    console.log(`  write ${DROPIN_FILE}`);

    // Migrate: remove OCO entries from settings.json if present
    if (existsSync(settingsPath)) {
      const existing = safeReadJson(settingsPath);
      const cleaned = removeOcoSettings(existing, {
        hooks: Object.keys(fragment.hooks || {}),
        mcpServers: Object.keys(fragment.mcpServers || {}),
        permissionsAllow: fragment.permissions?.allow || [],
      });
      if (Object.keys(cleaned).length === 0) {
        rmSync(settingsPath);
        console.log(`  rm    settings.json (migrated to drop-in)`);
      } else if (JSON.stringify(cleaned) !== JSON.stringify(existing)) {
        atomicWriteFileSync(settingsPath, JSON.stringify(cleaned, null, 2) + '\n');
        console.log(`  clean settings.json (migrated OCO entries to drop-in)`);
      }
    }
  } else {
    // Fallback: merge into settings.json (pre-v2.1.83 Claude Code)
    const existing = existsSync(settingsPath)
      ? safeReadJson(settingsPath)
      : {};

    const merged = mergeSettings(existing, fragment);
    atomicWriteFileSync(settingsPath, JSON.stringify(merged, null, 2) + '\n');
    console.log(`  merge settings.json (managed-settings.d not available)`);
  }

  // 4. Write .mcp.json (project-level only) — ensures `claude mcp list` visibility
  //    managed-settings.d/ activates MCP in-session, but `claude mcp list` reads .mcp.json
  if (!isGlobal) {
    const projectRoot = findProjectRoot(process.cwd());
    const mcpJsonPath = join(projectRoot, '.mcp.json');
    // Resolve bridge path to absolute so it works from any subdirectory
    const mcpEntry = JSON.parse(JSON.stringify(fragment.mcpServers || {}));
    if (mcpEntry.oco?.args?.[0]) {
      mcpEntry.oco.args[0] = join(targetDir, 'mcp/bridge.cjs');
    }

    if (existsSync(mcpJsonPath)) {
      // Merge: add oco key without overwriting other servers
      const existing = safeReadJson(mcpJsonPath);
      const merged = { ...existing, mcpServers: { ...(existing.mcpServers || {}), ...mcpEntry } };
      if (JSON.stringify(merged) !== JSON.stringify(existing)) {
        atomicWriteFileSync(mcpJsonPath, JSON.stringify(merged, null, 2) + '\n');
        console.log(`  merge .mcp.json (added oco server)`);
      } else {
        console.log(`  skip  .mcp.json (oco already declared)`);
      }
    } else {
      atomicWriteFileSync(mcpJsonPath, JSON.stringify({ mcpServers: mcpEntry }, null, 2) + '\n');
      console.log(`  write .mcp.json`);
    }
  }

  // 5. Write manifest
  const manifest = {
    version: VERSION,
    installedAt: new Date().toISOString(),
    global: isGlobal,
    settingsMode: useDropin ? 'managed-settings.d' : 'settings.json',
    files: pluginFiles,
    settingsKeys: {
      hooks: Object.keys(fragment.hooks || {}),
      mcpServers: Object.keys(fragment.mcpServers || {}),
      permissionsAllow: fragment.permissions?.allow || [],
    },
  };
  atomicWriteFileSync(join(targetDir, MANIFEST_FILE), JSON.stringify(manifest, null, 2) + '\n');

  console.log(`\n  Installed: ${copied} file(s), ${skipped} skipped.`);

  // 5. Post-install diagnostic — show real mode, not just files copied
  const ocoAvailable = commandExists('oco');
  const ocoVersion = ocoAvailable ? getOcoVersion() : null;
  const allHooksOk = ['hooks/pre-tool-use.mjs', 'hooks/post-tool-use.mjs',
    'hooks/stop.mjs', 'hooks/user-prompt-submit.cjs',
  ].every(f => existsSync(join(targetDir, f)));
  const bridgeOk = existsSync(join(targetDir, 'mcp/bridge.cjs'));
  const settingsOk = existsSync(join(targetDir, DROPIN_FILE)) || existsSync(settingsPath);

  const check = (ok, msg) => console.log(`    ${ok ? '✓' : '✗'} ${msg}`);

  console.log('\n  Plugin layer');
  check(allHooksOk, '4/4 hooks');
  check(settingsOk, useDropin ? 'managed-settings.d/50-oco.json' : 'settings.json');
  check(bridgeOk, 'MCP bridge');

  // Check .mcp.json
  if (!isGlobal) {
    const projectRoot = findProjectRoot(process.cwd());
    const mcpJsonPath = join(projectRoot, '.mcp.json');
    if (existsSync(mcpJsonPath)) {
      try {
        const mcpJson = JSON.parse(readFileSync(mcpJsonPath, 'utf8'));
        check(!!mcpJson.mcpServers?.oco, '.mcp.json');
      } catch { check(false, '.mcp.json (invalid JSON)'); }
    } else {
      check(false, '.mcp.json');
    }
  }

  console.log('\n  Runtime layer');
  if (ocoAvailable) {
    checkBinaryShadow(check);
  } else {
    check(false, 'oco binary not found');
  }

  // MCP live check
  console.log('\n  MCP health');
  const mcpStatus = checkMcpHealth();
  if (mcpStatus === 'connected') {
    check(true, 'claude mcp list — oco connected');
  } else if (mcpStatus === 'listed') {
    check(false, 'claude mcp list — oco listed but not connected');
  } else if (mcpStatus === 'missing') {
    check(false, 'claude mcp list — oco not found');
  } else {
    console.log(`    - claude mcp list — skipped (claude CLI not available)`);
  }

  // Dual install warning
  const otherDir = isGlobal ? resolveTargetSafe('project') : resolveTargetSafe('global');
  const otherManifest = readManifest(otherDir);
  if (otherManifest) {
    const otherScope = isGlobal ? 'project' : 'global';
    console.log(`\n    ⚠ Dual install: ${otherScope} also has v${otherManifest.version}`);
    console.log(`      Project takes precedence. Run: npx oco-claude-plugin uninstall ${otherScope === 'global' ? '--global' : ''}`);
  }

  // Mode
  const mode = resolveMode({ settingsOk, allHooksOk, bridgeOk, ocoAvailable });
  console.log(`\n  Mode: ${mode} (${MODE_DESCRIPTIONS[mode]})`);

  if (mode === 'plugin-only') {
    console.log(`
  What works now:
    • Safety hooks (destructive command blocking, verification enforcement)
    • 5 skills (/oco-inspect-repo-area, /oco-verify-fix, etc.)
    • 3 agents (codebase-investigator, patch-verifier, refactor-reviewer)
    • MCP verify_patch and working_memory (no binary needed)

  What needs the oco binary:
    • MCP search_codebase, trace_error, begin_task, collect_findings

  To install the runtime (requires Rust toolchain and access to OCO source):
    cd /path/to/oco && cargo install --path apps/dev-cli`);
  }
  console.log(`\n  Open Claude Code in this project to activate.\n`);
}

// --- Uninstall ---

async function uninstall() {
  const targetDir = resolveTarget();
  const manifestPath = join(targetDir, MANIFEST_FILE);

  console.log(`\n  OCO Claude Code Plugin — Uninstall`);
  console.log(`  Target: ${targetDir}\n`);

  if (!existsSync(manifestPath)) {
    console.log(`  No OCO installation found (no manifest).\n`);
    process.exit(0);
  }

  const manifest = safeReadJson(manifestPath, null);
  if (!manifest || !Array.isArray(manifest.files)) {
    console.error(`  ⚠ Manifest corrupted — deleting and re-running install will fix this.`);
    rmSync(manifestPath);
    process.exit(1);
  }

  // 1. Remove installed files
  let removed = 0;
  for (const relPath of manifest.files) {
    const fullPath = join(targetDir, relPath);
    if (existsSync(fullPath)) {
      rmSync(fullPath);
      removed++;
      console.log(`  rm    ${relPath}`);
    }
  }

  // 2. Clean empty directories (bottom-up, including parents)
  const allDirs = new Set();
  for (const f of manifest.files) {
    let d = dirname(f);
    while (d && d !== '.') {
      allDirs.add(d);
      d = dirname(d);
    }
  }
  const sortedDirs = [...allDirs].sort((a, b) => b.length - a.length);
  for (const dir of sortedDirs) {
    const fullDir = join(targetDir, dir);
    if (existsSync(fullDir) && isDirEmpty(fullDir)) {
      rmSync(fullDir, { recursive: true });
      console.log(`  rmdir ${dir}/`);
    }
  }

  // 3. Clean settings
  const dropinPath = join(targetDir, DROPIN_FILE);
  if (manifest.settingsMode === 'managed-settings.d' && existsSync(dropinPath)) {
    // Drop-in mode: just delete the fragment file
    rmSync(dropinPath);
    console.log(`  rm    ${DROPIN_FILE}`);
    // Clean empty managed-settings.d/ directory
    const dropinDir = dirname(dropinPath);
    if (existsSync(dropinDir) && isDirEmpty(dropinDir)) {
      rmSync(dropinDir, { recursive: true });
      console.log(`  rmdir managed-settings.d/`);
    }
  } else {
    // Legacy mode: clean settings.json
    const settingsPath = join(targetDir, 'settings.json');
    if (existsSync(settingsPath)) {
      const settings = safeReadJson(settingsPath);
      const cleaned = removeOcoSettings(settings, manifest.settingsKeys);

      if (Object.keys(cleaned).length === 0) {
        rmSync(settingsPath);
        console.log(`  rm    settings.json (empty after cleanup)`);
      } else {
        atomicWriteFileSync(settingsPath, JSON.stringify(cleaned, null, 2) + '\n');
        console.log(`  clean settings.json`);
      }
    }
  }

  // 4. Clean .mcp.json (remove oco server entry)
  if (!isGlobal) {
    const projectRoot = findProjectRoot(process.cwd());
    const mcpJsonPath = join(projectRoot, '.mcp.json');
    if (existsSync(mcpJsonPath)) {
      try {
        const mcpJson = safeReadJson(mcpJsonPath);
        if (mcpJson.mcpServers?.oco) {
          delete mcpJson.mcpServers.oco;
          if (Object.keys(mcpJson.mcpServers).length === 0) delete mcpJson.mcpServers;
          if (Object.keys(mcpJson).length === 0 || (Object.keys(mcpJson).length === 1 && mcpJson.mcpServers && Object.keys(mcpJson.mcpServers).length === 0)) {
            rmSync(mcpJsonPath);
            console.log(`  rm    .mcp.json (empty after cleanup)`);
          } else {
            atomicWriteFileSync(mcpJsonPath, JSON.stringify(mcpJson, null, 2) + '\n');
            console.log(`  clean .mcp.json (removed oco server)`);
          }
        }
      } catch {}
    }
  }

  // 5. Remove manifest
  rmSync(manifestPath);
  console.log(`  rm    ${MANIFEST_FILE}`);

  // 6. Remove .claude/ if empty
  if (existsSync(targetDir) && isDirEmpty(targetDir)) {
    rmSync(targetDir, { recursive: true });
    console.log(`  rmdir .claude/`);
  }

  console.log(`\n  Done! ${removed} file(s) removed.\n`);
}

// --- Status ---

function status() {
  const targetDir = resolveTarget();
  const manifestPath = join(targetDir, MANIFEST_FILE);

  console.log(`\n  OCO Claude Code Plugin — Status`);
  console.log(`  Target: ${targetDir}\n`);

  if (!existsSync(manifestPath)) {
    console.log(`  Not installed.\n`);
    process.exit(0);
  }

  const manifest = safeReadJson(manifestPath, null);
  if (!manifest || !Array.isArray(manifest.files)) {
    console.error(`  ⚠ Manifest corrupted — run: npx oco-claude-plugin install --force`);
    process.exit(1);
  }
  const present = manifest.files.filter(f => existsSync(join(targetDir, f)));
  const missing = manifest.files.filter(f => !existsSync(join(targetDir, f)));
  const ocoAvailable = commandExists('oco');

  console.log(`  Version:    ${manifest.version}`);
  console.log(`  Installed:  ${manifest.installedAt}`);
  console.log(`  Scope:      ${manifest.global ? 'global (~/.claude)' : 'project'}`);
  console.log(`  Settings:   ${manifest.settingsMode || 'settings.json'}`);
  console.log(`  Files:      ${present.length}/${manifest.files.length} present`);
  if (missing.length > 0) {
    console.log(`  Missing:    ${missing.join(', ')}`);
  }
  const ocoVersion = ocoAvailable ? getOcoVersion() : null;
  console.log(`  Runtime:    ${ocoAvailable ? `found${ocoVersion ? ` (v${ocoVersion})` : ''}` : 'not installed'}`);

  const allHooksOk = ['hooks/pre-tool-use.mjs', 'hooks/post-tool-use.mjs',
    'hooks/stop.mjs', 'hooks/user-prompt-submit.cjs',
  ].every(f => existsSync(join(targetDir, f)));
  const settingsOk = existsSync(join(targetDir, DROPIN_FILE)) || existsSync(join(targetDir, 'settings.json'));
  const bridgeOk = existsSync(join(targetDir, 'mcp/bridge.cjs'));
  const mode = resolveMode({ settingsOk, allHooksOk, bridgeOk, ocoAvailable });
  console.log(`  Mode:       ${mode}`);
  console.log();
}

// --- Doctor ---

function doctor() {
  const projectDir = resolveTargetSafe('project');
  const globalDir = resolveTargetSafe('global');

  console.log(`\n  OCO Claude Code Plugin — Doctor\n`);

  const issues = { errors: 0, warnings: 0 };
  const ok = (msg) => console.log(`    ✓ ${msg}`);
  const warn = (msg) => { console.log(`    ⚠ ${msg}`); issues.warnings++; };
  const fail = (msg) => { console.log(`    ✗ ${msg}`); issues.errors++; };

  // --- Environment ---
  console.log('  Environment');
  const nodeVer = process.version;
  const nodeMajor = parseInt(nodeVer.slice(1), 10);
  if (nodeMajor >= 18) ok(`Node.js ${nodeVer}`);
  else fail(`Node.js ${nodeVer} (>= 18 required)`);

  const claudeVer = getClaudeVersion();
  if (claudeVer) {
    const dropinOk = claudeVer.major > 2 || (claudeVer.major === 2 && claudeVer.minor > 1)
      || (claudeVer.major === 2 && claudeVer.minor === 1 && claudeVer.patch >= 83);
    if (dropinOk) ok(`Claude Code v${claudeVer.raw} (managed-settings.d supported)`);
    else warn(`Claude Code v${claudeVer.raw} (managed-settings.d requires >= 2.1.83)`);
  } else {
    fail('Claude Code not found (claude --version failed)');
  }

  // --- Installation source ---
  console.log('\n  Installation');
  const projectManifest = readManifest(projectDir);
  const globalManifest = readManifest(globalDir);
  let source = 'none';
  if (projectManifest && globalManifest) source = 'both';
  else if (projectManifest) source = 'project';
  else if (globalManifest) source = 'global';

  if (source === 'none') {
    fail('Not installed (no manifest found)');
    console.log(`\n  Mode: not installed`);
    console.log(`\n  Run: npx oco-claude-plugin install\n`);
    return 2;
  }

  const targetDir = source === 'global' ? globalDir : projectDir;
  const manifest = source === 'global' ? globalManifest : projectManifest;

  ok(`v${manifest.version} installed ${manifest.installedAt?.slice(0, 10) || '(unknown date)'}`);
  ok(`Source: ${source}`);

  if (source === 'both') {
    warn(`Dual install detected (global v${globalManifest.version} + project v${projectManifest.version})`);
    console.log(`      Project takes precedence. Run: npx oco-claude-plugin uninstall --global`);
  }

  // --- Settings ---
  const dropinPath = join(targetDir, DROPIN_FILE);
  const settingsPath = join(targetDir, 'settings.json');
  if (existsSync(dropinPath)) ok(`${DROPIN_FILE}`);
  else if (existsSync(settingsPath)) warn('settings.json (legacy mode, prefer managed-settings.d)');
  else fail('No settings found (hooks will not load)');

  // --- Plugin layer ---
  console.log('\n  Plugin layer');
  const expectedHooks = [
    'hooks/pre-tool-use.mjs', 'hooks/post-tool-use.mjs',
    'hooks/stop.mjs', 'hooks/user-prompt-submit.cjs',
  ];
  const expectedSkills = [
    'skills/oco/SKILL.md',
    'skills/oco-inspect-repo-area/SKILL.md', 'skills/oco-investigate-bug/SKILL.md',
    'skills/oco-safe-refactor/SKILL.md', 'skills/oco-trace-stack/SKILL.md',
    'skills/oco-verify-fix/SKILL.md',
  ];
  const expectedAgents = [
    'agents/codebase-investigator.md', 'agents/patch-verifier.md',
    'agents/refactor-reviewer.md',
  ];

  const checkGroup = (label, files) => {
    const present = files.filter(f => existsSync(join(targetDir, f)));
    const missing = files.filter(f => !existsSync(join(targetDir, f)));
    if (missing.length === 0) ok(`${present.length}/${files.length} ${label}`);
    else {
      fail(`${present.length}/${files.length} ${label} (missing: ${missing.join(', ')})`);
    }
  };

  checkGroup('hooks', expectedHooks);
  checkGroup('skills', expectedSkills);
  checkGroup('agents', expectedAgents);

  if (existsSync(join(targetDir, 'hooks/lib/utils.mjs'))) ok('hooks/lib/utils.mjs');
  else warn('hooks/lib/utils.mjs missing');

  if (existsSync(join(targetDir, 'mcp/bridge.cjs'))) ok('MCP bridge');
  else warn('MCP bridge missing (mcp/bridge.cjs)');

  // Check .mcp.json visibility
  if (source !== 'global') {
    const projectRoot = findProjectRoot(process.cwd());
    const mcpJsonPath = join(projectRoot, '.mcp.json');
    if (existsSync(mcpJsonPath)) {
      try {
        const mcpJson = JSON.parse(readFileSync(mcpJsonPath, 'utf8'));
        if (mcpJson.mcpServers?.oco) ok('.mcp.json (oco server declared — visible in claude mcp list)');
        else warn('.mcp.json exists but missing oco entry — run: npx oco-claude-plugin install --force');
      } catch { warn('.mcp.json exists but is not valid JSON'); }
    } else {
      warn('.mcp.json not found — oco will work in-session but not appear in claude mcp list');
      console.log('      Run: npx oco-claude-plugin install --force');
    }
  }

  // --- Version match ---
  if (manifest.version !== VERSION) {
    warn(`Installed v${manifest.version}, available v${VERSION} — run: npx oco-claude-plugin install --force`);
  }

  // --- Runtime layer ---
  console.log('\n  Runtime layer');
  const ocoAvailable = commandExists('oco');
  if (ocoAvailable) {
    const doctorCheck = (passed, msg) => passed ? ok(msg) : warn(msg);
    checkBinaryShadow(doctorCheck);
    const ocoVersion = getOcoVersion();
    const ocoUsable = checkOcoUsable();
    if (ocoUsable) {
      ok(`oco binary functional`);
    } else {
      warn(`oco binary found${ocoVersion ? ` (v${ocoVersion})` : ''} but returned an error`);
      console.log('      Run: oco --help  to diagnose');
    }
  } else {
    warn('oco binary not found');
    console.log('      MCP tools search_codebase, trace_error, begin_task, collect_findings');
    console.log('      will return fallback results without the runtime.');
    console.log('      To install (requires Rust toolchain + OCO source repo):');
    console.log('        cd /path/to/oco && cargo install --path apps/dev-cli');
  }

  // --- Conflicts ---
  console.log('\n  Conflicts');
  if (source !== 'both') ok('No global/project duplicates');

  // Check for orphan files
  const orphanMjs = join(targetDir, 'hooks/user-prompt-submit.mjs');
  if (existsSync(orphanMjs)) warn('Orphan hooks/user-prompt-submit.mjs found (only .cjs is active)');
  const legacyScripts = join(targetDir, 'hooks/scripts');
  if (existsSync(legacyScripts)) warn('Legacy hooks/scripts/ directory found (unused)');

  // --- Mode ---
  const allHooksOk = expectedHooks.every(f => existsSync(join(targetDir, f)));
  const settingsOk = existsSync(dropinPath) || existsSync(settingsPath);
  const bridgeOk = existsSync(join(targetDir, 'mcp/bridge.cjs'));

  const mode = resolveMode({ settingsOk, allHooksOk, bridgeOk, ocoAvailable });
  console.log(`\n  Mode: ${mode} (${MODE_DESCRIPTIONS[mode]})`);

  if (mode === 'broken' || mode === 'incomplete') {
    console.log(`\n  Run: npx oco-claude-plugin repair`);
  }

  console.log();

  // Exit codes: 0 = ok, 1 = warnings/incomplete, 2 = broken
  if (mode === 'broken') return 2;
  if (issues.warnings > 0 || mode === 'incomplete') return 1;
  return 0;
}

function readManifest(dir) {
  const p = join(dir, MANIFEST_FILE);
  try { return JSON.parse(readFileSync(p, 'utf8')); } catch { return null; }
}

function resolveTargetSafe(kind) {
  if (kind === 'global') return join(homedir(), '.claude');
  try {
    const root = findProjectRoot(process.cwd());
    return join(root, '.claude');
  } catch {
    return join(process.cwd(), '.claude');
  }
}

function getClaudeVersion() {
  try {
    const res = spawnSync('claude', ['--version'], {
      encoding: 'utf8', timeout: 3000, windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const version = (res.stdout || '').trim();
    const match = version.match(/\bv?(\d+)\.(\d+)\.(\d+)\b/);
    if (match) {
      const [, major, minor, patch] = match.map(Number);
      return { major, minor, patch, raw: `${major}.${minor}.${patch}` };
    }
  } catch {}
  return null;
}

// --- Repair ---

async function repair() {
  const targetDir = resolveTarget();
  const fragment = JSON.parse(readFileSync(join(PLUGIN_SRC, 'settings-fragment.json'), 'utf8'));

  console.log(`\n  OCO Claude Code Plugin — Repair${isDryRun ? ' (dry run)' : ''}`);
  console.log(`  Target: ${targetDir}\n`);

  if (isGlobal) rewritePathsForGlobal(fragment, targetDir);

  let repaired = 0;
  let ok = 0;

  const expectedFiles = collectFiles(PLUGIN_SRC).filter(f => f !== 'settings-fragment.json');

  // 1. Check and restore missing/damaged plugin files
  for (const relPath of expectedFiles) {
    const dest = join(targetDir, relPath);
    const src = join(PLUGIN_SRC, relPath);

    if (existsSync(dest)) {
      console.log(`    ✓ ${relPath}`);
      ok++;
      continue;
    }

    if (isDryRun) {
      console.log(`    ⚡ ${relPath} (would restore)`);
      repaired++;
      continue;
    }

    const destDir = dirname(dest);
    if (!existsSync(destDir)) mkdirSync(destDir, { recursive: true });

    try {
      writeFileSync(dest, readFileSync(src));
      console.log(`    ⚡ ${relPath} (restored)`);
      repaired++;
    } catch (err) {
      console.log(`    ✗ ${relPath} (restore failed: ${err.code || err.message})`);
    }
  }

  // 2. Restore settings if missing
  const dropinPath = join(targetDir, DROPIN_FILE);
  const settingsPath = join(targetDir, 'settings.json');

  if (existsSync(dropinPath)) {
    console.log(`    ✓ ${DROPIN_FILE}`);
    ok++;
  } else if (existsSync(settingsPath)) {
    console.log(`    ✓ settings.json (legacy mode)`);
    ok++;
  } else {
    if (isDryRun) {
      console.log(`    ⚡ ${DROPIN_FILE} (would restore)`);
      repaired++;
    } else {
      const dropinDir = join(targetDir, 'managed-settings.d');
      if (!existsSync(dropinDir)) mkdirSync(dropinDir, { recursive: true });
      atomicWriteFileSync(dropinPath, JSON.stringify(fragment, null, 2) + '\n');
      console.log(`    ⚡ ${DROPIN_FILE} (restored)`);
      repaired++;
    }
  }

  // 3. Restore manifest if missing
  const manifestPath = join(targetDir, MANIFEST_FILE);
  if (existsSync(manifestPath)) {
    ok++;
  } else {
    if (isDryRun) {
      console.log(`    ⚡ ${MANIFEST_FILE} (would restore)`);
      repaired++;
    } else {
      const manifest = {
        version: VERSION,
        installedAt: new Date().toISOString(),
        global: isGlobal,
        settingsMode: existsSync(dropinPath) ? 'managed-settings.d' : 'settings.json',
        files: expectedFiles,
        settingsKeys: {
          hooks: Object.keys(fragment.hooks || {}),
          mcpServers: Object.keys(fragment.mcpServers || {}),
          permissionsAllow: fragment.permissions?.allow || [],
        },
      };
      atomicWriteFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
      console.log(`    ⚡ ${MANIFEST_FILE} (restored)`);
      repaired++;
    }
  }

  // 4. Warn about dual install (never auto-fix)
  const otherDir = isGlobal ? resolveTargetSafe('project') : resolveTargetSafe('global');
  const otherManifest = readManifest(otherDir);
  if (otherManifest) {
    const otherScope = isGlobal ? 'project' : 'global';
    console.log(`\n    ⚠ Dual install: ${otherScope} also has v${otherManifest.version}`);
    console.log(`      To remove: npx oco-claude-plugin uninstall ${otherScope === 'global' ? '--global' : ''}`);
  }

  if (repaired === 0) {
    console.log(`\n  All ${ok} component(s) OK. Nothing to repair.`);
  } else if (isDryRun) {
    console.log(`\n  ${repaired} file(s) would be restored. Run without --dry-run to apply.`);
  } else {
    console.log(`\n  ${repaired} file(s) repaired, ${ok} already OK.`);
    console.log(`  Run 'npx oco-claude-plugin doctor' to verify.`);
  }

  console.log();
  return repaired > 0 && !isDryRun ? 0 : (repaired > 0 ? 1 : 0);
}

// --- Helpers ---

function usage() {
  console.log(`
  OCO Claude Code Plugin v${VERSION}

  Usage:
    oco-plugin install   [--global] [--force]   Install plugin
    oco-plugin uninstall [--global]              Remove plugin
    oco-plugin status    [--global]              Check installation
    oco-plugin doctor    [--global]              Diagnose installation
    oco-plugin repair    [--global] [--dry-run]  Fix common issues

  Options:
    --global, -g   Target ~/.claude/ (all projects)
    --force, -f    Overwrite existing files
    --dry-run      Show what would be fixed without changing anything

  Examples:
    npx oco-claude-plugin install          # project-level
    npx oco-claude-plugin install -g       # global
    npx oco-claude-plugin doctor           # check health
    npx oco-claude-plugin repair           # fix issues
    npx oco-claude-plugin uninstall        # clean removal
`);
}

function resolveTarget() {
  if (isGlobal) {
    return join(homedir(), '.claude');
  }
  const root = findProjectRoot(process.cwd());
  return join(root, '.claude');
}

function findProjectRoot(dir) {
  const markers = ['package.json', 'Cargo.toml', 'pyproject.toml', 'go.mod', '.git'];
  let current = resolve(dir);

  while (true) {
    for (const marker of markers) {
      if (existsSync(join(current, marker))) {
        return current;
      }
    }
    const parent = dirname(current);
    if (parent === current) return dir; // reached filesystem root
    current = parent;
  }
}

function collectFiles(dir, base = '') {
  const entries = readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const rel = base ? `${base}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      files.push(...collectFiles(join(dir, entry.name), rel));
    } else {
      files.push(rel);
    }
  }
  return files;
}

function mergeSettings(existing, fragment) {
  const result = { ...existing };

  // Merge hooks: append OCO entries if not already present
  if (fragment.hooks) {
    result.hooks = result.hooks || {};
    for (const [event, entries] of Object.entries(fragment.hooks)) {
      const existingEntries = result.hooks[event] || [];
      for (const entry of entries) {
        const cmd = entry.hooks?.[0]?.command || '';
        const alreadyExists = existingEntries.some(
          e => e.hooks?.some(h => h.command === cmd)
        );
        if (!alreadyExists) {
          existingEntries.push(entry);
        }
      }
      result.hooks[event] = existingEntries;
    }
  }

  // Merge MCP servers: add/update only the 'oco' key
  if (fragment.mcpServers) {
    result.mcpServers = result.mcpServers || {};
    for (const [key, value] of Object.entries(fragment.mcpServers)) {
      result.mcpServers[key] = value;
    }
  }

  // Merge permissions: union the allow array
  if (fragment.permissions?.allow) {
    result.permissions = result.permissions || {};
    result.permissions.allow = result.permissions.allow || [];
    for (const perm of fragment.permissions.allow) {
      if (!result.permissions.allow.includes(perm)) {
        result.permissions.allow.push(perm);
      }
    }
  }

  // Merge enabledPlugins
  if (fragment.enabledPlugins) {
    result.enabledPlugins = result.enabledPlugins || {};
    Object.assign(result.enabledPlugins, fragment.enabledPlugins);
  }

  return result;
}

function removeOcoSettings(settings, keys) {
  const result = { ...settings };

  // Remove OCO hooks
  if (result.hooks && keys.hooks) {
    for (const event of keys.hooks) {
      if (result.hooks[event]) {
        result.hooks[event] = result.hooks[event].filter(
          entry => !entry.hooks?.some(h =>
            (h.command || '').includes('.claude/hooks/')
          )
        );
        if (result.hooks[event].length === 0) {
          delete result.hooks[event];
        }
      }
    }
    if (Object.keys(result.hooks).length === 0) delete result.hooks;
  }

  // Remove OCO MCP servers
  if (result.mcpServers && keys.mcpServers) {
    for (const key of keys.mcpServers) {
      delete result.mcpServers[key];
    }
    if (Object.keys(result.mcpServers).length === 0) delete result.mcpServers;
  }

  // Remove OCO permissions
  if (result.permissions?.allow && keys.permissionsAllow) {
    result.permissions.allow = result.permissions.allow.filter(
      p => !keys.permissionsAllow.includes(p)
    );
    if (result.permissions.allow.length === 0) {
      delete result.permissions.allow;
    }
    if (Object.keys(result.permissions).length === 0) delete result.permissions;
  }

  return result;
}

function rewritePathsForGlobal(fragment, targetDir) {
  // For global install, hook commands need absolute paths
  const absPrefix = targetDir.replace(/\\/g, '/');

  if (fragment.hooks) {
    for (const entries of Object.values(fragment.hooks)) {
      for (const entry of entries) {
        for (const hook of entry.hooks || []) {
          if (hook.command) {
            hook.command = hook.command.replace(
              /\.claude\//g,
              absPrefix + '/'
            );
          }
        }
      }
    }
  }

  if (fragment.mcpServers?.oco?.args) {
    fragment.mcpServers.oco.args = fragment.mcpServers.oco.args.map(
      a => a.replace(/^\.claude\//, absPrefix + '/')
    );
  }
}

function supportsDropin(targetDir) {
  if (existsSync(join(targetDir, 'managed-settings.d'))) return true;
  const ver = getClaudeVersion();
  if (!ver) return false;
  return ver.major > 2 || (ver.major === 2 && ver.minor > 1)
    || (ver.major === 2 && ver.minor === 1 && ver.patch >= 83);
}

function isDirEmpty(dir) {
  try {
    return readdirSync(dir).length === 0;
  } catch {
    return true;
  }
}

function commandExists(cmd) {
  try {
    const check = process.platform === 'win32' ? `where ${cmd} 2>NUL` : `command -v ${cmd}`;
    execSync(check, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    return true;
  } catch {
    return false;
  }
}

/**
 * Find all `oco` binaries in PATH and return their paths + versions.
 * Detects shadow conflicts (old binary taking priority over new one).
 */
function findAllOcoBinaries() {
  const results = [];
  try {
    const cmd = process.platform === 'win32' ? 'where oco 2>NUL' : 'which -a oco 2>/dev/null';
    const out = execSync(cmd, { encoding: 'utf8', timeout: 3000, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] }).trim();
    const paths = out.split(/\r?\n/).filter(Boolean);
    for (const p of paths) {
      let version = null;
      try {
        const res = spawnSync(p, ['--version'], { encoding: 'utf8', timeout: 3000, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
        const match = (res.stdout || '').trim().match(/\b(\d+\.\d+\.\d+)/);
        version = match ? match[1] : null;
      } catch { /* ignore */ }
      results.push({ path: p, version });
    }
  } catch { /* no oco found at all */ }
  return results;
}

/**
 * Check for binary shadow issues and print warnings.
 * Returns the active (first in PATH) binary info.
 */
function checkBinaryShadow(check) {
  const bins = findAllOcoBinaries();
  if (bins.length === 0) return null;

  const active = bins[0];
  if (bins.length > 1) {
    // Multiple binaries — check for version mismatch
    const newest = bins.reduce((a, b) => {
      if (!a.version) return b;
      if (!b.version) return a;
      return a.version > b.version ? a : b;
    });
    if (newest.version && active.version && newest.version !== active.version) {
      check(false, `oco binary shadow detected — v${active.version} active, v${newest.version} available`);
      console.log(`      Active:  ${active.path} (v${active.version})`);
      console.log(`      Shadowed: ${newest.path} (v${newest.version})`);
      console.log(`      Fix: remove old binary or reorder PATH`);
      if (process.platform === 'win32') {
        console.log(`      Hint: del "${active.path}" (or move to end of PATH)`);
      } else {
        console.log(`      Hint: rm "${active.path}" (or reorder PATH in ~/.bashrc)`);
      }
      return active;
    }
  }

  // No shadow issue
  check(true, `oco binary found (v${active.version || '?'}) — ${active.path}`);
  return active;
}
