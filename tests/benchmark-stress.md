# OCO Stress Test — Limites de l'orchestration

> Ce benchmark pousse OCO a ses limites. Il teste des scenarios ou sans
> orchestration structuree, un agent se perdrait dans la complexite.
>
> Prerequis :
> - `oco` v0.4.0 sur le PATH
> - Un projet Rust reel (pas juste `cargo init`). Cloner un repo moyen :
>   `git clone https://github.com/serde-rs/serde /tmp/stress-target`
>   ou utiliser le repo OCO lui-meme comme workspace.
> - Provider : `stub` pour la mecanique, `anthropic` pour le vrai test.

---

## Prompt a coller

```
Tu es en mode stress test. Execute chaque scenario dans l'ordre.
Pour chaque test, reporte :
- Le comportement observe (actions, steps, routing, erreurs)
- La coherence du resultat (est-ce que OCO a fait quelque chose d'intelligent ?)
- PASS / FAIL / PARTIAL selon les criteres

NE CORRIGE RIEN. Reporte seulement.

IMPORTANT : entre chaque phase, execute `oco runs list 2>&1 | head -5`
pour verifier que les artifacts s'accumulent correctement.

---

### Phase 1 — Cascade de complexite croissante

Execute ces 6 commandes dans l'ordre exact. Le but est de verifier que
le classifier et le router reagissent correctement a un gradient de
complexite, et que le systeme ne se "confond" pas entre les runs.

```bash
oco run "what does println do" --workspace . --provider stub 2>&1
oco run "find all structs that implement Display" --workspace . --provider stub 2>&1
oco run "refactor error handling to use thiserror" --workspace . --provider stub 2>&1
oco run "add comprehensive test coverage for the parser module including edge cases and fuzzing" --workspace . --provider stub 2>&1
oco run "redesign the entire storage layer: migrate from SQLite to PostgreSQL, add connection pooling, implement read replicas, add migration system, and ensure zero-downtime deployment" --workspace . --provider stub 2>&1
oco run "audit the full codebase for security vulnerabilities including injection attacks, authentication bypass, race conditions, timing attacks, and supply chain risks, then produce a detailed remediation plan with priority ranking" --workspace . --provider stub 2>&1
```

Attendu :
| # | Requete (abregee) | Complexite attendue | Routing attendu |
|---|-------------------|---------------------|-----------------|
| 1 | println | Trivial | flat loop |
| 2 | find structs Display | Trivial | flat loop |
| 3 | refactor error thiserror | Medium | plan engine |
| 4 | test coverage parser fuzzing | Medium/High | plan engine |
| 5 | redesign storage layer... | High/Critical | plan engine |
| 6 | audit full codebase security... | High/Critical | plan engine |

Reporte : pour chacun, la complexite obtenue, le routing, et l'action.
Critere PASS : au moins 4/6 routes correctement (flat vs plan engine).

---

### Phase 2 — Bombardement rapide (10 runs en <30s)

Le but est de verifier que le systeme de traces ne se melange pas,
que les IDs sont uniques, et que les artifacts sont integres sous charge.

```bash
for i in $(seq 1 10); do
  oco run "task number $i: analyze module $i" --workspace . --provider stub 2>&1 | tail -3
done
```

Puis verifie :
```bash
oco runs list 2>&1
ls .oco/runs/ | wc -l
```

Attendu :
- 10 nouveaux runs apparaissent
- Chaque run a un ID unique
- Aucun summary.json corrompu (pas de JSON invalide)
- Total runs >= 16 (6 phase 1 + 10 phase 2)

Verification d'integrite :
```bash
for dir in .oco/runs/*/; do
  python3 -c "import json; json.load(open('${dir}summary.json'))" 2>&1 && echo "OK: $dir" || echo "CORRUPT: $dir"
done
```

Critere PASS : 0 corrupt, >= 16 runs, IDs tous uniques.

---

### Phase 3 — Requetes adversariales

Ces requetes testent les cas limites du classifier et du router.
Certaines sont volontairement ambigues, malformees, ou trompeuses.

3.1. Injection de complexite fausse :
```bash
oco run "just print hello world (but make it production-grade with error handling, logging, metrics, tracing, and distributed systems support)" --workspace . --provider stub 2>&1
```
Attendu : classifier doit gerer les mots-cles contradictoires.
Reporte : complexite, routing, coherence.

3.2. Requete en francais :
```bash
oco run "refactoriser le module d'authentification pour utiliser des tokens JWT avec rotation automatique" --workspace . --provider stub 2>&1
```
Attendu : le classifier utilise des mots-cles anglais — le francais
peut degrader la classification. Reporte le comportement.

3.3. Requete tres longue (>200 mots) :
```bash
oco run "I need you to analyze the complete authentication flow starting from the login endpoint through the middleware chain into the session manager then check how tokens are validated including JWT signature verification expiration checking audience validation issuer verification then trace how the refresh token rotation works and verify that old tokens are properly invalidated after rotation also check if there are any race conditions in concurrent token refresh scenarios and verify that the token blacklist is properly synchronized across multiple server instances then check the password hashing implementation to ensure it uses bcrypt or argon2 with appropriate cost factors and verify that timing-safe comparison is used for all secret comparisons finally audit the CORS configuration and CSP headers to ensure they follow current best practices for preventing XSS and CSRF attacks" --workspace . --provider stub 2>&1
```
Attendu : High ou Critical (requete longue + mots-cles security).
Reporte : complexite, routing.

3.4. Caracteres speciaux et unicode :
```bash
oco run "fix the bug in src/lib.rs:42 — the 'parse_résumé' function crashes on UTF-8 input like 'café' or '日本語'" --workspace . --provider stub 2>&1
```
Attendu : pas de crash, traitement normal.
Reporte : comportement, exit code.

3.5. Requete qui ressemble a du code :
```bash
oco run "fn main() { let x = vec![1,2,3]; println!(\"{:?}\", x); }" --workspace . --provider stub 2>&1
```
Attendu : pas de crash. Classifier peut etre confus mais ne doit pas planter.

Critere PASS : 0 crash/panic sur les 5 tests, routing coherent sur au moins 3/5.

---

### Phase 4 — Endurance et budget

4.1. Run long avec budget par defaut :
```bash
oco run "perform a complete architecture review of this codebase including dependency analysis, module coupling metrics, API surface audit, and produce a detailed refactoring roadmap" --workspace . --provider stub 2>&1
```
Reporte : nombre de steps, tokens_used, duree, status final.

4.2. Verifier que TOUS les runs de la session sont consultables :
```bash
oco runs list 2>&1
```
Reporte : nombre total, aucun "orphelin" ou erreur.

4.3. Replay du run le plus complexe :
```bash
oco runs show last 2>&1
```
Reporte : la trace est lisible, contient les actions, pas de corruption.

4.4. Statistiques globales :
```bash
echo "=== Total runs ===" && ls .oco/runs/ | wc -l
echo "=== Disk usage ===" && du -sh .oco/runs/
echo "=== Largest trace ===" && wc -l .oco/runs/*/trace.jsonl | sort -n | tail -3
```
Reporte : nombre de runs, taille disque, plus grosse trace.

Critere PASS : tous les runs consultables, pas de corruption, traces lisibles.

---

### Phase 5 — Le boss final (vrai LLM)

Ce test utilise un vrai LLM pour valider que l'orchestration produit
des resultats exploitables (pas juste du stub).

**Option A — Claude Code (defaut, recommande) :**
```bash
oco run "analyze the error handling patterns in this codebase, identify inconsistencies between modules, and suggest a unified approach" --workspace . 2>&1
```
Note : `--provider claude-code` est le defaut, pas besoin de le specifier.

**Option B — GPT-5.4 via MCP reviewer :**
Si claude CLI n'est pas disponible, utilise le MCP `gpt54-reviewer`
(review_code) de Claude Code pour soumettre du code reel a GPT-5.4.
Lis les fichiers error.rs des crates et soumets-les a review_code.

**Option C — Anthropic API directe :**
```bash
oco run "analyze the error handling patterns..." --workspace . --provider anthropic 2>&1
```
Necessite `ANTHROPIC_API_KEY`.

5.1. Attendu :
- Le plan engine est active (Medium+)
- Le LLM produit une vraie reponse (pas un stub)
- La reponse mentionne des fichiers/patterns reels du workspace
- tokens_used > 0 dans le summary
- La reponse est coherente et utile
Reporte : provider utilise, routing, qualite (1-5), tokens_used.

5.2. Comparaison stub vs vrai LLM :
```bash
oco run "explain the architecture of this project" --workspace . --provider stub 2>&1
oco run "explain the architecture of this project" --workspace . 2>&1
```
Reporte : differences de comportement, qualite relative.

Critere PASS : reponse coherente du LLM, tokens_used > 0, pas de crash.

---

### Tableau recapitulatif

| Phase | Tests | Critere | Resultat | Detail |
|-------|-------|---------|----------|--------|
| 1. Cascade complexite | 6 runs | >= 4/6 routing correct | PASS/FAIL | ... |
| 2. Bombardement rapide | 10 runs | 0 corrupt, >= 16 total | PASS/FAIL | ... |
| 3. Adversarial | 5 tests | 0 crash, >= 3/5 coherent | PASS/FAIL | ... |
| 4. Endurance | 4 checks | 0 corruption, traces OK | PASS/FAIL | ... |
| 5. Boss final (vrai LLM) | 2 tests | reponse coherente, tokens > 0 | PASS/FAIL/SKIP | ... |

Score final : X/5 phases PASS (ou X/4 si phase 5 SKIP)

### Ce que ce test prouve

Sans orchestration structuree, un agent face a ces requetes :
- Ne sait pas differencier "explain X" de "redesign the entire storage layer"
- Traite tout avec le meme budget et la meme strategie
- Ne persiste aucune trace exploitable
- Ne survit pas au bombardement (pas d'isolation entre runs)
- Ne peut pas replayer un run passe pour comprendre ce qui s'est passe

OCO transforme le chaos en signal : chaque requete est classifiee,
routee, budgetee, tracee, et rejouable. C'est la difference entre
un agent qui "fait des trucs" et un systeme d'orchestration.
```
