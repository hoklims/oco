<script lang="ts">
  import type { MissionMemory } from './types'

  let { mission }: { mission: MissionMemory } = $props()

  type Tab = 'facts' | 'hypotheses' | 'questions' | 'plan' | 'risks'
  let activeTab = $state<Tab>('facts')

  const TABS: { id: Tab; label: string; count: () => number }[] = [
    { id: 'facts', label: 'Facts', count: () => mission.facts.length },
    { id: 'hypotheses', label: 'Hypotheses', count: () => mission.hypotheses.length },
    { id: 'questions', label: 'Questions', count: () => mission.open_questions.length },
    { id: 'plan', label: 'Plan', count: () => (mission.plan.completed_steps.length + mission.plan.remaining_steps.length) },
    { id: 'risks', label: 'Risks', count: () => mission.risks.length },
  ]

  function confidenceColor(pct: number): string {
    if (pct >= 80) return 'text-green'
    if (pct >= 50) return 'text-amber'
    return 'text-red'
  }

  function confidenceBar(pct: number): string {
    if (pct >= 80) return '#34d399'
    if (pct >= 50) return '#fbbf24'
    return '#f87171'
  }
</script>

<div class="flex flex-col h-full">
  <!-- Mission summary -->
  <div class="px-4 py-2.5 border-b border-border bg-surface-2 shrink-0">
    <div class="text-xs font-mono text-text-3 uppercase tracking-widest mb-1">Mission Memory</div>
    <div class="text-sm text-text-1 font-medium truncate">{mission.mission}</div>
    {#if mission.plan.phase}
      <div class="text-xs text-cyan font-mono mt-1">Phase: {mission.plan.phase}</div>
    {/if}
  </div>

  <!-- Tab bar -->
  <div class="flex border-b border-border shrink-0">
    {#each TABS as tab}
      {@const count = tab.count()}
      <button
        onclick={() => activeTab = tab.id}
        class="flex-1 px-2 py-2 text-xs font-mono transition-colors relative
          {activeTab === tab.id ? 'text-text-1 bg-surface-2' : 'text-text-3 hover:text-text-2'}"
      >
        {tab.label}
        {#if count > 0}
          <span class="ml-1 text-[10px] {activeTab === tab.id ? 'text-cyan' : 'text-text-3'}">{count}</span>
        {/if}
        {#if activeTab === tab.id}
          <div class="absolute bottom-0 left-2 right-2 h-px bg-cyan"></div>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Tab content -->
  <div class="flex-1 overflow-y-auto p-3 space-y-2">
    {#if activeTab === 'facts'}
      {#if mission.facts.length === 0}
        <div class="text-xs text-text-3 opacity-50">No verified facts yet</div>
      {:else}
        {#each mission.facts as fact, i}
          <div class="px-3 py-2 rounded bg-surface-2 border border-border space-y-1">
            <div class="text-xs text-text-1 leading-relaxed">{fact.content}</div>
            <div class="flex items-center gap-2 text-[10px] text-text-3">
              {#if fact.source}
                <span class="font-mono text-cyan truncate max-w-40">{fact.source}</span>
              {/if}
              <span class="ml-auto font-mono">{new Date(fact.established_at).toLocaleTimeString()}</span>
            </div>
          </div>
        {/each}
      {/if}

    {:else if activeTab === 'hypotheses'}
      {#if mission.hypotheses.length === 0}
        <div class="text-xs text-text-3 opacity-50">No active hypotheses</div>
      {:else}
        {#each mission.hypotheses as hyp}
          <div class="px-3 py-2 rounded bg-surface-2 border border-border space-y-1.5">
            <div class="text-xs text-text-1 leading-relaxed">{hyp.content}</div>
            <div class="flex items-center gap-2">
              <div class="flex-1 h-1.5 rounded-full bg-surface-3 overflow-hidden">
                <div class="h-full rounded-full transition-all" style="width: {hyp.confidence_pct}%; background: {confidenceBar(hyp.confidence_pct)}"></div>
              </div>
              <span class="text-xs font-mono {confidenceColor(hyp.confidence_pct)}">{hyp.confidence_pct}%</span>
            </div>
            {#if hyp.supporting_evidence.length > 0}
              <div class="space-y-0.5">
                {#each hyp.supporting_evidence as ev}
                  <div class="text-[10px] text-text-3 pl-2 border-l border-border">{ev}</div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/if}

    {:else if activeTab === 'questions'}
      {#if mission.open_questions.length === 0}
        <div class="text-xs text-text-3 opacity-50">No open questions</div>
      {:else}
        {#each mission.open_questions as q, i}
          <div class="flex items-start gap-2 px-3 py-2 rounded bg-surface-2 border border-border">
            <span class="text-amber text-xs font-mono shrink-0">Q{i + 1}</span>
            <span class="text-xs text-text-2 leading-relaxed">{q}</span>
          </div>
        {/each}
      {/if}

    {:else if activeTab === 'plan'}
      {#if mission.plan.current_objective}
        <div class="px-3 py-2 rounded bg-cyan-dim border border-cyan/20">
          <div class="text-[10px] text-text-3 uppercase tracking-wider mb-0.5">Objective</div>
          <div class="text-xs text-cyan">{mission.plan.current_objective}</div>
        </div>
      {/if}
      {#if mission.plan.completed_steps.length > 0}
        <div class="space-y-1">
          <div class="text-[10px] text-text-3 uppercase tracking-wider">Completed</div>
          {#each mission.plan.completed_steps as step}
            <div class="flex items-center gap-2 px-3 py-1.5 rounded bg-surface-2 text-xs">
              <span class="pip pip-done"></span>
              <span class="text-text-2 line-through opacity-60">{step}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if mission.plan.remaining_steps.length > 0}
        <div class="space-y-1">
          <div class="text-[10px] text-text-3 uppercase tracking-wider">Remaining</div>
          {#each mission.plan.remaining_steps as step, i}
            <div class="flex items-center gap-2 px-3 py-1.5 rounded bg-surface-2 text-xs">
              <span class="pip {i === 0 ? 'pip-active' : 'pip-idle'}"></span>
              <span class="text-text-1">{step}</span>
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'risks'}
      {#if mission.risks.length === 0}
        <div class="text-xs text-text-3 opacity-50">No identified risks</div>
      {:else}
        {#each mission.risks as risk}
          <div class="flex items-start gap-2 px-3 py-2 rounded bg-red-dim border border-red/10">
            <span class="text-red text-xs shrink-0">!</span>
            <span class="text-xs text-text-2 leading-relaxed">{risk}</span>
          </div>
        {/each}
      {/if}

      <!-- Key decisions (shown under risks tab) -->
      {#if mission.key_decisions.length > 0}
        <div class="pt-2 space-y-1">
          <div class="text-[10px] text-text-3 uppercase tracking-wider">Key Decisions</div>
          {#each mission.key_decisions as decision}
            <div class="flex items-start gap-2 px-3 py-1.5 rounded bg-surface-2 border border-border">
              <span class="text-purple text-xs shrink-0">&rarr;</span>
              <span class="text-xs text-text-2">{decision}</span>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <!-- Footer: verification status -->
  <div class="px-4 py-2 border-t border-border bg-surface shrink-0 flex items-center gap-3 text-[10px] font-mono">
    <span class="uppercase text-text-3">Verification</span>
    <span class="text-green">{mission.verification.checks_passed.length} passed</span>
    {#if mission.verification.checks_failed.length > 0}
      <span class="text-red">{mission.verification.checks_failed.length} failed</span>
    {/if}
    {#if mission.verification.unverified_files.length > 0}
      <span class="text-amber">{mission.verification.unverified_files.length} unverified</span>
    {/if}
    <span class="ml-auto text-text-3">{mission.modified_files.length} files modified</span>
  </div>
</div>
