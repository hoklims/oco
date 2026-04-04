<script lang="ts">
  let { onLaunch, onDemo }: {
    onLaunch: (seed: string, workspace: string, provider: string, model: string) => void
    onDemo: () => void
  } = $props()

  let seed = $state('')
  let workspace = $state('')
  let provider = $state('claude-code')
  let model = $state('sonnet')
  let launching = $state(false)
  let error = $state<string | null>(null)
  let showAdvanced = $state(false)

  // Connection check
  let serverStatus = $state<'checking' | 'online' | 'offline'>('checking')

  async function checkServer() {
    try {
      const res = await fetch('/health')
      serverStatus = res.ok ? 'online' : 'offline'
    } catch {
      serverStatus = 'offline'
    }
  }

  // Check on mount
  checkServer()

  async function handleLaunch() {
    if (!seed.trim()) return
    launching = true
    error = null
    try {
      onLaunch(seed.trim(), workspace.trim(), provider, model)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Launch failed'
      launching = false
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && seed.trim()) {
      handleLaunch()
    }
  }

  const EXAMPLES = [
    { label: 'Bug fix', seed: 'Fix the intermittent 500 error on /api/search when under concurrent load (>50 req/s). Stack trace points to SQLite connection pool lock contention.' },
    { label: 'Feature', seed: 'Add JWT authentication with refresh token rotation to replace the current session-based auth. Preserve backward compatibility for 1 release cycle.' },
    { label: 'Refactor', seed: 'Migrate the retrieval layer from synchronous SQLite to async with tokio-rusqlite. Maintain the existing FTS5 full-text search interface.' },
    { label: 'Audit', seed: 'Audit the error handling across all HTTP endpoints. Identify silent failures, missing error propagation, and catch blocks that swallow errors.' },
  ]
</script>

<div class="h-full flex items-center justify-center p-6">
  <div class="w-full max-w-2xl space-y-6">
    <!-- Header -->
    <div class="text-center space-y-2">
      <div class="text-[10px] font-mono text-text-3 uppercase tracking-[0.3em]">OCO</div>
      <h1 class="text-2xl text-text-1 font-medium tracking-tight">Mission Control</h1>
      <p class="text-sm text-text-3">Describe your project and task. OCO handles the rest.</p>
    </div>

    <!-- Server status -->
    <div class="flex justify-center">
      <div class="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-surface-2 border border-border text-[10px] font-mono">
        <span class="w-1.5 h-1.5 rounded-full {serverStatus === 'online' ? 'bg-green' : serverStatus === 'offline' ? 'bg-red' : 'bg-amber animate-pulse'}"></span>
        <span class="text-text-3">
          {serverStatus === 'online' ? 'oco serve connected' : serverStatus === 'offline' ? 'oco serve not detected' : 'checking...'}
        </span>
        {#if serverStatus === 'offline'}
          <button onclick={checkServer} class="text-blue hover:text-text-1 ml-1">retry</button>
        {/if}
      </div>
    </div>

    <!-- Seed input -->
    <div class="space-y-2">
      <label class="text-[10px] font-mono text-text-3 uppercase tracking-wider block">Mission Seed</label>
      <textarea
        bind:value={seed}
        onkeydown={handleKeydown}
        placeholder="Describe your project context and task...&#10;&#10;Example: Fix the authentication bug in the login flow. The app uses Express.js with PostgreSQL. Users report intermittent 401 errors after token refresh."
        rows={6}
        class="w-full bg-surface-2 border border-border rounded-lg px-4 py-3 text-sm text-text-1 font-mono
          placeholder:text-text-3/40 resize-none outline-none
          focus:border-cyan/40 focus:ring-1 focus:ring-cyan/20 transition-all"
      ></textarea>
      <div class="flex justify-between items-center">
        <span class="text-[10px] font-mono text-text-3">{seed.length} chars</span>
        <span class="text-[10px] font-mono text-text-3">Ctrl+Enter to launch</span>
      </div>
    </div>

    <!-- Quick examples -->
    <div class="flex flex-wrap gap-1.5">
      {#each EXAMPLES as ex}
        <button
          onclick={() => { seed = ex.seed }}
          class="px-2.5 py-1 rounded-full text-[10px] font-mono bg-surface-2 border border-border
            text-text-3 hover:text-text-1 hover:border-border-2 transition-colors"
        >
          {ex.label}
        </button>
      {/each}
    </div>

    <!-- Advanced settings -->
    <div>
      <button
        onclick={() => showAdvanced = !showAdvanced}
        class="text-[10px] font-mono text-text-3 hover:text-text-2 flex items-center gap-1 transition-colors"
      >
        <span class="transition-transform {showAdvanced ? 'rotate-90' : ''}">&rsaquo;</span>
        Configuration
      </button>

      {#if showAdvanced}
        <div class="mt-3 grid grid-cols-2 gap-3">
          <!-- Workspace -->
          <div class="col-span-2">
            <label class="text-[10px] font-mono text-text-3 uppercase tracking-wider block mb-1">Workspace Path</label>
            <input
              bind:value={workspace}
              type="text"
              placeholder="./  (current directory)"
              class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-xs font-mono text-text-2
                placeholder:text-text-3/40 outline-none focus:border-cyan/40 transition-colors"
            />
          </div>

          <!-- Provider -->
          <div>
            <label class="text-[10px] font-mono text-text-3 uppercase tracking-wider block mb-1">Provider</label>
            <select bind:value={provider}
              class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-xs font-mono text-text-2 outline-none">
              <option value="claude-code">Claude Code</option>
              <option value="anthropic">Anthropic API</option>
              <option value="ollama">Ollama (local)</option>
              <option value="stub">Stub (dev)</option>
            </select>
          </div>

          <!-- Model -->
          <div>
            <label class="text-[10px] font-mono text-text-3 uppercase tracking-wider block mb-1">Model</label>
            <select bind:value={model}
              class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-xs font-mono text-text-2 outline-none">
              <option value="sonnet">Sonnet</option>
              <option value="opus">Opus</option>
              <option value="haiku">Haiku</option>
            </select>
          </div>
        </div>
      {/if}
    </div>

    <!-- Error -->
    {#if error}
      <div class="px-4 py-2 rounded bg-red-dim border border-red/20 text-xs text-red font-mono">{error}</div>
    {/if}

    <!-- Actions -->
    <div class="flex items-center gap-3">
      <button
        onclick={handleLaunch}
        disabled={!seed.trim() || launching || serverStatus === 'checking'}
        class="flex-1 px-6 py-3 rounded-lg font-mono text-sm font-medium transition-all
          {!seed.trim() || launching || serverStatus === 'checking'
            ? 'bg-surface-3 text-text-3 cursor-not-allowed'
            : serverStatus === 'online'
              ? 'bg-cyan/15 text-cyan border border-cyan/30 hover:bg-cyan/25 hover:border-cyan/50'
              : 'bg-amber/15 text-amber border border-amber/30 hover:bg-amber/25'}"
      >
        {#if launching}
          <span class="animate-pulse">Launching...</span>
        {:else if serverStatus === 'offline'}
          Launch (server offline — will queue)
        {:else}
          Launch Mission
        {/if}
      </button>

      <button
        onclick={onDemo}
        class="px-4 py-3 rounded-lg text-xs font-mono text-text-3 bg-surface-2 border border-border
          hover:text-text-1 hover:bg-surface-3 transition-colors"
      >
        Demo
      </button>
    </div>

    <!-- Hint -->
    <div class="text-center">
      <p class="text-[10px] font-mono text-text-3/50">
        {serverStatus === 'online'
          ? 'Connected to OCO server. Your seed will be indexed, classified, and executed.'
          : 'Start the server with `oco serve --port 3000` for live orchestration.'}
      </p>
    </div>
  </div>
</div>
