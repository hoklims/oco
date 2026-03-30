<script lang="ts">
  let { text, variant = 'thought' }: {
    text: string
    variant?: 'thought' | 'action' | 'warning' | 'success'
  } = $props()

  let visible = $state(false)
  let gone = $state(false)

  const borderColor: Record<string, string> = {
    thought: 'border-border-2',
    action: 'border-cyan/30',
    warning: 'border-amber/30',
    success: 'border-green/30',
  }

  const textColor: Record<string, string> = {
    thought: 'text-text-2',
    action: 'text-cyan/80',
    warning: 'text-amber/80',
    success: 'text-green/80',
  }

  $effect(() => {
    const t1 = setTimeout(() => { visible = true }, 80)
    const t2 = setTimeout(() => { gone = true }, 5000)
    return () => { clearTimeout(t1); clearTimeout(t2) }
  })
</script>

{#if !gone}
  <div
    class="py-1.5 px-3 bg-surface-2/80 border-l-2 {borderColor[variant]} {textColor[variant]}
      text-[12px] leading-relaxed w-full rounded-r
      transition-all duration-500
      {visible ? 'opacity-100 translate-y-0' : 'opacity-0 -translate-y-1'}"
  >
    {text}
  </div>
{/if}
