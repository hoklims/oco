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

import { existsSync, mkdirSync, cpSync, readFileSync, writeFileSync, rmSync, readdirSync, statSync } from 'node:fs';
import { join, dirname, relative, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const PLUGIN_SRC = join(__dirname, 'plugin');
const MANIFEST_FILE = '.oco-install-manifest.json';
const VERSION = '0.1.0';

// --- CLI Argument Parsing ---

const [,, command, ...args] = process.argv;
const isGlobal = args.includes('--global') || args.includes('-g');
const isForce = args.includes('--force') || args.includes('-f');

switch (command) {
  case 'install':   await install(); break;
  case 'uninstall': await uninstall(); break;
  case 'status':    status(); break;
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

    cpSync(src, dest);
    copied++;
    console.log(`  copy  ${relPath}`);
  }

  // 3. Merge settings.json
  const fragment = JSON.parse(readFileSync(join(PLUGIN_SRC, 'settings-fragment.json'), 'utf8'));

  // Adjust paths for global install
  if (isGlobal) {
    rewritePathsForGlobal(fragment, targetDir);
  }

  const existing = existsSync(settingsPath)
    ? JSON.parse(readFileSync(settingsPath, 'utf8'))
    : {};

  const merged = mergeSettings(existing, fragment);
  writeFileSync(settingsPath, JSON.stringify(merged, null, 2) + '\n');
  console.log(`  merge settings.json`);

  // 4. Write manifest
  const manifest = {
    version: VERSION,
    installedAt: new Date().toISOString(),
    global: isGlobal,
    files: pluginFiles,
    settingsKeys: {
      hooks: Object.keys(fragment.hooks || {}),
      mcpServers: Object.keys(fragment.mcpServers || {}),
      permissionsAllow: fragment.permissions?.allow || [],
    },
  };
  writeFileSync(join(targetDir, MANIFEST_FILE), JSON.stringify(manifest, null, 2) + '\n');

  // 5. Check for oco binary
  const ocoAvailable = commandExists('oco');

  console.log(`\n  Done! ${copied} file(s) installed, ${skipped} skipped.`);
  if (!ocoAvailable) {
    console.log(`\n  Note: 'oco' binary not found on PATH.`);
    console.log(`  Hooks and skills work without it. For full MCP support:`);
    console.log(`    cargo install --path apps/dev-cli  (from the OCO repo)`);
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

  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));

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

  // 3. Clean settings.json
  const settingsPath = join(targetDir, 'settings.json');
  if (existsSync(settingsPath)) {
    const settings = JSON.parse(readFileSync(settingsPath, 'utf8'));
    const cleaned = removeOcoSettings(settings, manifest.settingsKeys);

    if (Object.keys(cleaned).length === 0) {
      rmSync(settingsPath);
      console.log(`  rm    settings.json (empty after cleanup)`);
    } else {
      writeFileSync(settingsPath, JSON.stringify(cleaned, null, 2) + '\n');
      console.log(`  clean settings.json`);
    }
  }

  // 4. Remove manifest
  rmSync(manifestPath);
  console.log(`  rm    ${MANIFEST_FILE}`);

  // 5. Remove .claude/ if empty
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

  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const present = manifest.files.filter(f => existsSync(join(targetDir, f)));
  const missing = manifest.files.filter(f => !existsSync(join(targetDir, f)));
  const ocoAvailable = commandExists('oco');

  console.log(`  Version:    ${manifest.version}`);
  console.log(`  Installed:  ${manifest.installedAt}`);
  console.log(`  Scope:      ${manifest.global ? 'global (~/.claude)' : 'project'}`);
  console.log(`  Files:      ${present.length}/${manifest.files.length} present`);
  if (missing.length > 0) {
    console.log(`  Missing:    ${missing.join(', ')}`);
  }
  console.log(`  OCO binary: ${ocoAvailable ? 'found' : 'not found (optional)'}`);
  console.log();
}

// --- Helpers ---

function usage() {
  console.log(`
  OCO Claude Code Plugin v${VERSION}

  Usage:
    oco-plugin install   [--global] [--force]   Install plugin
    oco-plugin uninstall [--global]              Remove plugin
    oco-plugin status    [--global]              Check installation

  Options:
    --global, -g   Install to ~/.claude/ (all projects)
    --force, -f    Overwrite existing files

  Examples:
    npx oco-claude-plugin install          # project-level
    npx oco-claude-plugin install -g       # global
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
  const { root } = new URL('file:///' + current.replace(/\\/g, '/'));

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
