<script lang="ts">
  import type { RunScorecard, GateResult, MissionMemory, ReviewPacket } from './types'
  import ScorecardRadar from './ScorecardRadar.svelte'
  import GateVerdict from './GateVerdict.svelte'
  import MissionPanel from './MissionPanel.svelte'
  import ReviewBadge from './ReviewBadge.svelte'

  let { review }: { review: ReviewPacket } = $props()

  type View = 'overview' | 'scorecard' | 'gate' | 'mission'
  let activeView = $state<View>('overview')

  // Derived summary stats
  let totalChecks = $derived(
    (review.verification.checks_passed.length + review.verification.checks_failed.length)
  )
  let riskCount = $derived(review.open_risks.risks.length + review.open_risks.open_questions.length)
</script>

<div class="h-full flex flex-col">
  <!-- Header with review badge + view tabs -->
  <div class="px-4 py-3 border-b border-border bg-surface shrink-0 space-y-2.5">
    <div class="flex items-center justify-between">
      <div>
        <div class="text-xs font-mono text-text-3 uppercase tracking-widest mb-1">Post-Run Intelligence</div>
        <ReviewBadge
          mergeReadiness={review.merge_readiness}
          gateVerdict={review.gate_verdict}
          trustVerdict={review.trust_verdict}
          freshness={review.baseline_freshness}
        />
      </div>
      {#if review.scorecard}
        <div class="text-right">
          <div class="text-2xl font-mono font-bold {review.scorecard.overall_score >= 0.8 ? 'text-green' : review.scorecard.overall_score >= 0.6 ? 'text-amber' : 'text-red'}">
            {(review.scorecard.overall_score * 100).toFixed(0)}
          </div>
          <div class="text-[10px] text-text-3 font-mono">composite score</div>
        </div>
      {/if}
    </div>

    <!-- View tabs -->
    <div class="flex gap-1">
      {#each [
        { id: 'overview' as View, label: 'Overview' },
        { id: 'scorecard' as View, label: 'Scorecard', disabled: !review.scorecard },
        { id: 'gate' as View, label: 'Gate', disabled: !review.gate_result },
        { id: 'mission' as View, label: 'Mission' },
      ] as tab}
        <button
          onclick={() => { if (!tab.disabled) activeView = tab.id }}
          disabled={tab.disabled}
          class="px-3 py-1.5 text-xs font-mono rounded transition-colors
            {activeView === tab.id ? 'bg-surface-3 text-text-1 ring-1 ring-border-2' :
             tab.disabled ? 'text-text-3 opacity-30 cursor-not-allowed' :
             'text-text-3 hover:text-text-2 hover:bg-surface-2'}"
        >
          {tab.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Content area -->
  <div class="flex-1 overflow-y-auto">
    {#if activeView === 'overview'}
      <div class="p-4 space-y-4">
        <!-- Quick stats grid -->
        <div class="grid grid-cols-4 gap-3">
          <div class="px-3 py-2.5 rounded-lg bg-surface-2 border border-border text-center">
            <div class="text-lg font-mono text-text-1">{review.changes.modified_files.length}</div>
            <div class="text-[10px] text-text-3 uppercase">Files</div>
          </div>
          <div class="px-3 py-2.5 rounded-lg bg-surface-2 border border-border text-center">
            <div class="text-lg font-mono {review.verification.checks_failed.length === 0 ? 'text-green' : 'text-red'}">{totalChecks}</div>
            <div class="text-[10px] text-text-3 uppercase">Checks</div>
          </div>
          <div class="px-3 py-2.5 rounded-lg bg-surface-2 border border-border text-center">
            <div class="text-lg font-mono text-text-1">{review.changes.key_decisions.length}</div>
            <div class="text-[10px] text-text-3 uppercase">Decisions</div>
          </div>
          <div class="px-3 py-2.5 rounded-lg bg-surface-2 border border-border text-center">
            <div class="text-lg font-mono {riskCount > 0 ? 'text-amber' : 'text-green'}">{riskCount}</div>
            <div class="text-[10px] text-text-3 uppercase">Risks</div>
          </div>
        </div>

        <!-- Narrative -->
        {#if review.changes.narrative}
          <div class="px-4 py-3 rounded-lg bg-surface-2 border border-border">
            <div class="text-[10px] text-text-3 uppercase tracking-wider mb-1.5">Summary</div>
            <div class="text-sm text-text-2 leading-relaxed">{review.changes.narrative}</div>
          </div>
        {/if}

        <!-- Verification summary -->
        <div class="space-y-2">
          <div class="text-[10px] text-text-3 uppercase tracking-wider">Verification</div>
          {#each review.verification.checks_passed as check}
            <div class="flex items-center gap-2 px-3 py-1.5 rounded bg-green-dim border border-green/10">
              <span class="pip pip-done"></span>
              <span class="text-xs text-green font-mono">{check}</span>
            </div>
          {/each}
          {#each review.verification.checks_failed as check}
            <div class="flex items-center gap-2 px-3 py-1.5 rounded bg-red-dim border border-red/10">
              <span class="pip pip-fail"></span>
              <span class="text-xs text-red font-mono">{check}</span>
            </div>
          {/each}
        </div>

        <!-- Modified files -->
        {#if review.changes.modified_files.length > 0}
          <div class="space-y-1">
            <div class="text-[10px] text-text-3 uppercase tracking-wider">Modified Files</div>
            <div class="flex flex-wrap gap-1">
              {#each review.changes.modified_files as file}
                <span class="px-2 py-0.5 rounded bg-surface-3 text-[10px] font-mono text-text-2 border border-border">{file}</span>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Key decisions -->
        {#if review.changes.key_decisions.length > 0}
          <div class="space-y-1.5">
            <div class="text-[10px] text-text-3 uppercase tracking-wider">Key Decisions</div>
            {#each review.changes.key_decisions as decision}
              <div class="flex items-start gap-2 px-3 py-1.5 rounded bg-surface-2 border border-border">
                <span class="text-purple text-xs shrink-0">&rarr;</span>
                <span class="text-xs text-text-2">{decision}</span>
              </div>
            {/each}
          </div>
        {/if}

        <!-- Open risks -->
        {#if review.open_risks.risks.length > 0}
          <div class="space-y-1.5">
            <div class="text-[10px] text-text-3 uppercase tracking-wider">Open Risks</div>
            {#each review.open_risks.risks as risk}
              <div class="flex items-start gap-2 px-3 py-1.5 rounded bg-red-dim border border-red/10">
                <span class="text-red text-xs shrink-0">!</span>
                <span class="text-xs text-text-2">{risk}</span>
              </div>
            {/each}
          </div>
        {/if}

        <!-- Unverified files -->
        {#if review.verification.unverified_files.length > 0}
          <div class="space-y-1">
            <div class="text-[10px] text-text-3 uppercase tracking-wider">Unverified Files</div>
            <div class="flex flex-wrap gap-1">
              {#each review.verification.unverified_files as file}
                <span class="px-2 py-0.5 rounded bg-amber-dim text-[10px] font-mono text-amber border border-amber/10">{file}</span>
              {/each}
            </div>
          </div>
        {/if}
      </div>

    {:else if activeView === 'scorecard' && review.scorecard}
      <div class="p-4">
        <ScorecardRadar scorecard={review.scorecard} />
      </div>

    {:else if activeView === 'gate' && review.gate_result}
      <div class="p-4">
        <GateVerdict result={review.gate_result} />
      </div>

    {:else if activeView === 'mission'}
      <!-- Build a MissionMemory from ReviewPacket data -->
      {@const missionData = {
        schema_version: 1,
        session_id: review.run_id,
        created_at: review.generated_at,
        mission: review.changes.narrative ?? 'Orchestrated task',
        facts: review.verification.checks_passed.map(c => ({ content: c, source: 'verifier', established_at: review.generated_at })),
        hypotheses: [],
        open_questions: review.open_risks.open_questions,
        plan: { current_objective: null, completed_steps: review.changes.key_decisions, remaining_steps: [], phase: 'complete' },
        verification: {
          freshness: review.baseline_freshness?.freshness ?? 'Unknown',
          unverified_files: review.verification.unverified_files,
          last_check: review.generated_at,
          checks_passed: review.verification.checks_passed,
          checks_failed: review.verification.checks_failed,
        },
        modified_files: review.changes.modified_files,
        key_decisions: review.changes.key_decisions,
        risks: review.open_risks.risks,
      } satisfies MissionMemory}
      <MissionPanel mission={missionData} />
    {/if}
  </div>
</div>
