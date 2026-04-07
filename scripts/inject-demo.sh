#!/bin/bash
# Inject a full demo event sequence into a tracking session.
# Usage: ./scripts/inject-demo.sh <session_id> [port]

SID="${1:?Usage: $0 <session_id> [port]}"
PORT="${2:-3000}"
URL="http://127.0.0.1:$PORT/api/v1/dashboard/sessions/$SID/events"
H="Content-Type: application/json"
OK=0
FAIL=0

post() {
  local label="$1"; shift
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$URL" -H "$H" -d "$1")
  if [ "$code" = "204" ]; then
    OK=$((OK + 1))
  else
    FAIL=$((FAIL + 1))
    echo "FAIL [$code]: $label"
  fi
}

S1="aaaa0001-0000-0000-0000-000000000000"
S2="aaaa0002-0000-0000-0000-000000000000"
S3="aaaa0003-0000-0000-0000-000000000000"
S4="aaaa0004-0000-0000-0000-000000000000"
S5="aaaa0005-0000-0000-0000-000000000000"

post "run_started" '{"type":"run_started","provider":"claude-code","model":"opus","request_summary":"Analyser Chroma Cloud vs ChromaDB local","complexity":"medium - research + architecture decision"}'

post "classify" '{"type":"flat_step_completed","step":1,"action_type":"classifying","reason":"medium - research + architecture decision","duration_ms":2500,"budget_snapshot":{"tokens_used":1200,"tokens_remaining":48800,"tool_calls_used":2,"tool_calls_remaining":28,"retrievals_used":1,"verify_cycles_used":0,"elapsed_secs":3}}'

post "explore" '{"type":"plan_exploration","candidates":[{"strategy":"speed","step_count":3,"estimated_tokens":24000,"verify_count":2,"parallel_groups":3,"score":0.62,"winner":false,"planning_tokens":800},{"strategy":"safety","step_count":5,"estimated_tokens":32000,"verify_count":3,"parallel_groups":3,"score":0.81,"winner":true,"planning_tokens":1200}],"winner_strategy":"safety","winner_score":0.81}'

post "plan_generated" "{\"type\":\"plan_generated\",\"plan_id\":\"00000001-0000-0000-0000-000000000000\",\"step_count\":5,\"parallel_group_count\":3,\"critical_path_length\":4,\"estimated_total_tokens\":32000,\"strategy\":\"Competitive (safety won)\",\"team\":null,\"steps\":[{\"id\":\"$S1\",\"name\":\"Search OSS solutions\",\"description\":\"Find Chroma libraries\",\"role\":\"researcher\",\"execution_mode\":\"subagent\",\"depends_on\":[],\"verify_after\":false,\"estimated_tokens\":2000,\"preferred_model\":\"haiku\"},{\"id\":\"$S2\",\"name\":\"Architecture analysis\",\"description\":\"Compare Cloud vs Local\",\"role\":\"architect\",\"execution_mode\":\"inline\",\"depends_on\":[\"$S1\"],\"verify_after\":false,\"estimated_tokens\":5000,\"preferred_model\":\"opus\"},{\"id\":\"$S3\",\"name\":\"Implement migration\",\"description\":\"Migrate to Local\",\"role\":\"implementer\",\"execution_mode\":\"inline\",\"depends_on\":[\"$S2\"],\"verify_after\":true,\"estimated_tokens\":12000,\"preferred_model\":null},{\"id\":\"$S4\",\"name\":\"Update config\",\"description\":\"Update ChromaDB config\",\"role\":\"implementer\",\"execution_mode\":\"inline\",\"depends_on\":[\"$S2\"],\"verify_after\":true,\"estimated_tokens\":6000,\"preferred_model\":null},{\"id\":\"$S5\",\"name\":\"Integration tests\",\"description\":\"Full test suite\",\"role\":\"tester\",\"execution_mode\":\"inline\",\"depends_on\":[\"$S3\",\"$S4\"],\"verify_after\":true,\"estimated_tokens\":7000,\"preferred_model\":null}]}"

# Step execution
post "start-search" "{\"type\":\"step_started\",\"step_id\":\"$S1\",\"step_name\":\"Search OSS solutions\",\"role\":\"researcher\",\"execution_mode\":\"subagent\"}"
post "done-search" "{\"type\":\"step_completed\",\"step_id\":\"$S1\",\"step_name\":\"Search OSS solutions\",\"success\":true,\"duration_ms\":2500,\"tokens_used\":1800,\"detail_ref\":null}"
post "progress-1" '{"type":"progress","completed":1,"total":5,"active_steps":[],"budget":{"tokens_used":4800,"tokens_remaining":45200,"tool_calls_used":6,"tool_calls_remaining":24,"retrievals_used":3,"verify_cycles_used":0,"elapsed_secs":8}}'

post "start-arch" "{\"type\":\"step_started\",\"step_id\":\"$S2\",\"step_name\":\"Architecture analysis\",\"role\":\"architect\",\"execution_mode\":\"inline\"}"
post "done-arch" "{\"type\":\"step_completed\",\"step_id\":\"$S2\",\"step_name\":\"Architecture analysis\",\"success\":true,\"duration_ms\":4200,\"tokens_used\":4600,\"detail_ref\":null}"
post "progress-2" '{"type":"progress","completed":2,"total":5,"active_steps":[],"budget":{"tokens_used":9400,"tokens_remaining":40600,"tool_calls_used":10,"tool_calls_remaining":20,"retrievals_used":4,"verify_cycles_used":0,"elapsed_secs":14}}'

# Parallel impl
post "start-impl" "{\"type\":\"step_started\",\"step_id\":\"$S3\",\"step_name\":\"Implement migration\",\"role\":\"implementer\",\"execution_mode\":\"inline\"}"
post "start-config" "{\"type\":\"step_started\",\"step_id\":\"$S4\",\"step_name\":\"Update config\",\"role\":\"implementer\",\"execution_mode\":\"inline\"}"

post "done-impl" "{\"type\":\"step_completed\",\"step_id\":\"$S3\",\"step_name\":\"Implement migration\",\"success\":true,\"duration_ms\":8500,\"tokens_used\":11500,\"detail_ref\":null}"
post "verify-impl" "{\"type\":\"verify_gate_result\",\"step_id\":\"$S3\",\"step_name\":\"Implement migration\",\"checks\":[{\"check_type\":\"build\",\"passed\":true,\"summary\":\"0 errors\"},{\"check_type\":\"test\",\"passed\":true,\"summary\":\"8 tests passed\"}],\"overall_passed\":true,\"replan_triggered\":false}"

post "done-config" "{\"type\":\"step_completed\",\"step_id\":\"$S4\",\"step_name\":\"Update config\",\"success\":true,\"duration_ms\":5200,\"tokens_used\":5800,\"detail_ref\":null}"
post "verify-config" "{\"type\":\"verify_gate_result\",\"step_id\":\"$S4\",\"step_name\":\"Update config\",\"checks\":[{\"check_type\":\"build\",\"passed\":true,\"summary\":\"0 errors\"},{\"check_type\":\"test\",\"passed\":true,\"summary\":\"3 tests passed\"}],\"overall_passed\":true,\"replan_triggered\":false}"

post "progress-4" '{"type":"progress","completed":4,"total":5,"active_steps":[],"budget":{"tokens_used":28500,"tokens_remaining":21500,"tool_calls_used":22,"tool_calls_remaining":8,"retrievals_used":5,"verify_cycles_used":2,"elapsed_secs":28}}'

# Integration tests
post "start-tests" "{\"type\":\"step_started\",\"step_id\":\"$S5\",\"step_name\":\"Integration tests\",\"role\":\"tester\",\"execution_mode\":\"inline\"}"
post "done-tests" "{\"type\":\"step_completed\",\"step_id\":\"$S5\",\"step_name\":\"Integration tests\",\"success\":true,\"duration_ms\":6000,\"tokens_used\":6800,\"detail_ref\":null}"
post "verify-tests" "{\"type\":\"verify_gate_result\",\"step_id\":\"$S5\",\"step_name\":\"Integration tests\",\"checks\":[{\"check_type\":\"build\",\"passed\":true,\"summary\":\"0 errors\"},{\"check_type\":\"test\",\"passed\":true,\"summary\":\"15 tests passed\"},{\"check_type\":\"lint\",\"passed\":true,\"summary\":\"0 warnings\"}],\"overall_passed\":true,\"replan_triggered\":false}"

post "progress-5" '{"type":"progress","completed":5,"total":5,"active_steps":[],"budget":{"tokens_used":36500,"tokens_remaining":13500,"tool_calls_used":28,"tool_calls_remaining":2,"retrievals_used":6,"verify_cycles_used":3,"elapsed_secs":36}}'

post "run_stopped" '{"type":"run_stopped","reason":"task_complete","total_steps":22,"total_tokens":36500}'

echo "Done: $OK OK, $FAIL failed"
