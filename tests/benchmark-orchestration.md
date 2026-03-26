# OCO Orchestration v2 — Benchmark Prompt

> Copie ce fichier dans un dossier contenant un projet Rust
> (`cargo init` suffit), puis lance Claude Code et colle le prompt ci-dessous.
> Le binaire `oco` doit etre sur le PATH (`cargo install --path apps/dev-cli`).

---

## Prompt a coller

```
Tu es en mode benchmark. Execute chaque test dans l'ordre, reporte le resultat exact (PASS / FAIL + detail si FAIL), et produis un tableau recapitulatif a la fin. Ne corrige rien toi-meme — reporte seulement.

### Phase 1 — Routing par complexite

1.1. Execute : `oco run "what is a mutex in Rust" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite classifiee : Trivial
     - Action executee : RESPOND (pas PLAN)
     - Passe par la flat loop (pas de mention "plan engine")
     Reporte : complexite affichee, action, presence/absence de "plan engine".

1.2. Execute : `oco run "find where the Config struct is defined" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite : Trivial ou Low
     - Action : RETRIEVE ou RESPOND (pas PLAN)
     - Passe par la flat loop
     Reporte : complexite, action, routing.

1.3. Execute : `oco run "refactor the auth module to use JWT tokens" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite : Medium
     - Action : PLAN
     - Mention "Medium+ task" ou "plan engine" dans la sortie
     - GraphRunner execute au moins 1 step
     Reporte : complexite, action, mention plan engine, nombre de steps.

1.4. Execute : `oco run "add a complete REST API with authentication, rate limiting, and database integration" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite : High ou Medium
     - Action : PLAN
     - Routing vers plan engine
     Reporte : complexite, action, routing.

1.5. Execute : `oco run "explain this code" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite : Trivial
     - Pas de PLAN, reste dans flat loop
     - Se termine normalement (pas de crash)
     Reporte : complexite, nombre de steps, status final.

### Phase 2 — Traces et artifacts

2.1. Execute : `oco run "refactor error handling across all modules" --workspace . --provider stub 2>&1`
     Puis : `oco runs show last 2>&1`
     Attendu :
     - La trace montre un routing PLAN
     - Le run se termine avec status Completed ou Stopped
     Reporte : actions dans la trace, status final.

2.2. Execute : `oco runs list 2>&1`
     Attendu :
     - Au moins 6 runs visibles (ceux des tests precedents)
     - Chaque run a un ID, timestamp, status
     Reporte : nombre de runs, format correct.

2.3. Lis le fichier summary.json du dernier run :
     ```
     cat .oco/runs/$(ls -t .oco/runs/ | head -1)/summary.json 2>/dev/null || echo "NO SUMMARY"
     ```
     Attendu :
     - Contient `complexity` (Medium ou High)
     - Contient `tokens_used` (> 0 pour un run plan engine)
     - Contient `status` et `duration_ms`
     Reporte : champs presents, valeurs de complexity et tokens_used.

### Phase 3 — Budget et robustesse

3.1. Execute : `oco run "rebuild the entire application from scratch with microservices architecture" --workspace . --provider stub 2>&1`
     Attendu :
     - Se termine sans crash ni panic
     - Budget non depasse (tokens_used <= max_total_tokens)
     - Nombre de steps raisonnable (pas de boucle infinie)
     Reporte : tokens_used, steps, status final.

3.2. Execute : `oco run "" --workspace . --provider stub 2>&1`
     Attendu :
     - Gere gracieusement une requete vide
     - Pas de crash / panic
     Reporte : comportement, exit code.

3.3. Execute : `oco run "fix the bug" --workspace . --provider stub 2>&1`
     Attendu :
     - Complexite : Low ou Medium (selon le classifier)
     - Se termine normalement
     Reporte : complexite, routing (flat loop ou plan engine), status.

### Phase 4 — Coherence du plan

4.1. Execute : `oco run "refactor the database layer to use connection pooling" --workspace . --provider stub 2>&1`
     Capture la sortie complete.
     Attendu :
     - Le plan contient au moins 1 step
     - Le step a un role (ex: refactorer, coder, architect)
     - Si verify_after est true, une verification est mentionnee
     Reporte : nombre de steps, role(s), verify gates.

4.2. Execute deux fois de suite la meme commande :
     ```
     oco run "add logging to all API endpoints" --workspace . --provider stub 2>&1
     oco run "add logging to all API endpoints" --workspace . --provider stub 2>&1
     ```
     Attendu :
     - Les deux runs produisent un comportement coherent (meme complexite, meme routing)
     - Chaque run a un ID different
     Reporte : complexite x2, routing x2, IDs differents.

---

### Tableau recapitulatif

| # | Test | Attendu | Resultat | Detail |
|---|------|---------|----------|--------|
| 1.1 | Trivial query | flat loop | PASS/FAIL | ... |
| 1.2 | Low search | flat loop | PASS/FAIL | ... |
| 1.3 | Medium refactor | plan engine | PASS/FAIL | ... |
| 1.4 | High feature | plan engine | PASS/FAIL | ... |
| 1.5 | Trivial explain | flat loop | PASS/FAIL | ... |
| 2.1 | Trace plan | PLAN visible | PASS/FAIL | ... |
| 2.2 | Runs list | >= 6 runs | PASS/FAIL | ... |
| 2.3 | Summary JSON | complexity + tokens | PASS/FAIL | ... |
| 3.1 | Budget stress | no crash | PASS/FAIL | ... |
| 3.2 | Empty request | graceful | PASS/FAIL | ... |
| 3.3 | Ambiguous task | no crash | PASS/FAIL | ... |
| 4.1 | Plan structure | role + steps | PASS/FAIL | ... |
| 4.2 | Determinism | coherent x2 | PASS/FAIL | ... |

Score final : X/13 PASS
```
