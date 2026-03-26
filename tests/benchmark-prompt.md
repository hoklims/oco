# OCO Plugin — Benchmark Prompt

> Copie ce fichier dans un dossier vierge contenant un petit projet
> (ex: `npm init -y && echo "console.log('hello')" > index.js`),
> puis lance Claude Code et colle le prompt ci-dessous.

---

## Prompt à coller

```
Tu es en mode benchmark. Exécute chaque étape dans l'ordre, reporte le résultat exact (PASS / FAIL + détail si FAIL), et produis un tableau récapitulatif à la fin. Ne corrige rien toi-même — reporte seulement.

### Phase 1 — Installation

1.1. Exécute : `npx oco-claude-plugin install`
     Attendu : message de succès, fichiers copiés, manifest créé.
     Reporte : exit code, nombre de fichiers copiés, présence de `.oco-install-manifest.json`.

1.2. Exécute : `npx oco-claude-plugin status`
     Attendu : statut "installed", liste des fichiers, version 0.4.0.
     Reporte : sortie complète.

1.3. Vérifie la structure créée :
     ```
     ls -R .claude/
     ```
     Attendu (minimum) :
     - `.claude/hooks/` — au moins 4 fichiers (user-prompt-submit.cjs, pre-tool-use.mjs, post-tool-use.mjs, stop.mjs)
     - `.claude/mcp/` — bridge.cjs ou bridge.js
     - `.claude/agents/` — 3 fichiers .md
     - `.claude/skills/` — 5 sous-dossiers (oco-inspect-repo-area, oco-investigate-bug, oco-safe-refactor, oco-trace-stack, oco-verify-fix)
     - `.claude/settings.json` — existe et contient les clés hooks, mcpServers, permissions
     Reporte : fichiers présents vs attendus, fichiers manquants.

### Phase 2 — Settings & Hooks

2.1. Lis `.claude/settings.json` et vérifie :
     - `hooks` contient 4 événements : UserPromptSubmit, PreToolUse, PostToolUse, Stop
     - `mcpServers` contient une entrée `oco` (ou `oco-bridge`)
     - `permissions.allow` contient au moins une entrée Bash pour oco
     Reporte : clés présentes/absentes, valeurs inattendues.

2.2. Lis `.claude/hooks/pre-tool-use.mjs` et vérifie :
     - Il bloque les commandes destructrices (rm -rf, git reset --hard, etc.)
     - Il ne crashe pas sur un import manquant (tout est `node:*`)
     Reporte : patterns détectés, imports externes éventuels.

2.3. Lis `.claude/hooks/stop.mjs` et vérifie :
     - Il vérifie qu'une commande de vérification (cargo test, npm test, etc.) a été exécutée si des fichiers source ont été modifiés
     - Il ne bloque PAS si aucun fichier source n'a été modifié
     Reporte : logique détectée, cas limites potentiels.

### Phase 3 — Skills

3.1. Liste les skills disponibles (regarde dans .claude/skills/*/SKILL.md).
     Pour chaque skill, reporte :
     - Nom
     - Présence du fichier SKILL.md
     - Le skill contient des instructions (non vide, > 10 lignes)

3.2. Vérifie que chaque skill a un trigger cohérent :
     - `/oco-verify-fix` → doit mentionner "build", "test", "lint"
     - `/oco-trace-stack` → doit mentionner "stacktrace", "error", "exception"
     - `/oco-investigate-bug` → doit mentionner "bug", "regression"
     - `/oco-safe-refactor` → doit mentionner "impact", "rename", "move"
     - `/oco-inspect-repo-area` → doit mentionner "explore", "understand", "module"
     Reporte : mots-clés trouvés/manquants par skill.

### Phase 4 — Agents

4.1. Lis chaque fichier dans `.claude/agents/` et vérifie :
     - `codebase-investigator.md` — mentionne exploration, Read, Grep, Glob
     - `patch-verifier.md` — mentionne vérification, test, build
     - `refactor-reviewer.md` — mentionne impact, breaking changes, références
     Reporte : contenu cohérent ou anomalies.

### Phase 5 — MCP Bridge

5.1. Lis `.claude/mcp/bridge.cjs` (ou bridge.js) et vérifie :
     - Il expose au moins 4 tools : search_codebase, trace_error, verify_patch, collect_findings
     - Il utilise stdio comme transport
     - Il ne crashe pas si `oco` n'est pas sur le PATH (graceful degradation)
     Reporte : tools trouvés, transport, gestion d'erreur.

### Phase 6 — Manifest & Idempotence

6.1. Lis `.oco-install-manifest.json` et vérifie :
     - Contient version, timestamp, scope, files (liste)
     - La version correspond à 0.4.0

6.2. Exécute à nouveau : `npx oco-claude-plugin install`
     Attendu : ne doit PAS écraser les fichiers existants (skip), pas d'erreur.
     Reporte : comportement (écrasement ou skip), exit code.

### Phase 7 — Uninstall & Cleanup

7.1. Exécute : `npx oco-claude-plugin uninstall`
     Attendu : suppression de tous les fichiers installés, nettoyage settings.json, suppression manifest.
     Reporte : exit code, fichiers restants.

7.2. Vérifie :
     - `.claude/hooks/` — vide ou supprimé
     - `.claude/mcp/` — vide ou supprimé
     - `.claude/agents/` — vide ou supprimé
     - `.claude/skills/` — vide ou supprimé
     - `.claude/settings.json` — nettoyé (plus de clés OCO) ou supprimé
     - `.oco-install-manifest.json` — supprimé
     Reporte : résidus éventuels.

---

### Tableau récapitulatif

Produis ce tableau à la fin :

| # | Test | Résultat | Détail |
|---|------|----------|--------|
| 1.1 | Install | PASS/FAIL | ... |
| 1.2 | Status | PASS/FAIL | ... |
| 1.3 | Structure | PASS/FAIL | ... |
| 2.1 | Settings | PASS/FAIL | ... |
| 2.2 | Hook pre-tool | PASS/FAIL | ... |
| 2.3 | Hook stop | PASS/FAIL | ... |
| 3.1 | Skills présence | PASS/FAIL | ... |
| 3.2 | Skills triggers | PASS/FAIL | ... |
| 4.1 | Agents | PASS/FAIL | ... |
| 5.1 | MCP bridge | PASS/FAIL | ... |
| 6.1 | Manifest | PASS/FAIL | ... |
| 6.2 | Idempotence | PASS/FAIL | ... |
| 7.1 | Uninstall | PASS/FAIL | ... |
| 7.2 | Cleanup | PASS/FAIL | ... |

Score final : X/14 PASS
```
