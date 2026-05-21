<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!--
  MikeRust mark — the 3×3 rust-gradient grid (src/assets/mikerust_logo_3x3.svg).
  `activity` runs a three-phase pulse over the grid and recolours it:
    · idle     — static, native rust palette
    · thinking — pulsing, rust (an LLM call is in flight)
    · docx     — pulsing, blue (generating a .docx from a template)
    · upload   — pulsing, green (extracting text from an uploaded file)

  Pulse choreography (positions 1-9 row-major: 1 2 3 / 4 5 6 / 7 8 9):
    phase 1 — cells 5, 6, 8, 9 (bottom-right 2×2 quadrant)
    phase 2 — cell 9 alone (the corner spark)
    phase 3 — all nine cells together (the full board)
    repeat

  Each cell gets a keyframe whose `scale(1.08)` peak lands inside the
  phases it belongs to and stays at the resting `scale(0.78)` outside,
  so the three classes interlock seamlessly on the same timeline.
  Honours prefers-reduced-motion (no animation).
-->
<script lang="ts">
  interface Props {
    size?: number
    activity?: 'idle' | 'thinking' | 'docx' | 'upload'
    class?: string
  }

  let { size = 40, activity = 'idle', class: extraClass = '' }: Props = $props()

  // Grid geometry mirrors mikerust_logo_3x3.svg (group translated to 250,250).
  const COORD = [-135, -45, 45]
  const FILLS = [
    ['#431407', '#7C2D0A', '#9A3412'],
    ['#7C2D0A', '#C2410C', '#EA580C'],
    ['#9A3412', '#EA580C', '#F97316'],
  ]

  // Tier classification per the spec:
  //   corner — position 9 (row 2, col 2): pulses in phases 1 + 2 + 3
  //   quad   — positions 5, 6, 8 (rest of the 2×2 quadrant): pulses in phases 1 + 3
  //   rest   — positions 1, 2, 3, 4, 7: pulses only in phase 3
  function tierFor(row: number, col: number): 'corner' | 'quad' | 'rest' {
    if (row === 2 && col === 2) return 'corner'
    if (row >= 1 && col >= 1) return 'quad'
    return 'rest'
  }

  const cells = COORD.flatMap((y, row) =>
    COORD.map((x, col) => ({
      x,
      y,
      fill: FILLS[row][col],
      tier: tierFor(row, col),
    })),
  )
</script>

<svg
  class="mike-logo mike-logo-{activity} {extraClass}"
  width={size}
  height={size}
  viewBox="105 105 280 280"
  role="img"
  aria-label="MikeRust"
>
  <g transform="translate(250,250)">
    {#each cells as c (`${c.x},${c.y}`)}
      <rect
        x={c.x}
        y={c.y}
        width="80"
        height="80"
        fill={c.fill}
        class="mike-logo-cell mike-logo-tier-{c.tier}"
      />
    {/each}
  </g>
</svg>
