import type { ComponentChildren } from 'preact';

/** 指标单元：数值 + 标签。数值用 mono 对齐，便于纵向比较。 */
export function Metric({
  value,
  label,
  accent,
  exact,
}: {
  value: ComponentChildren;
  label: string;
  accent?: string;
  exact?: string;
}) {
  return (
    <div class="min-w-0" title={exact}>
      <div
        class="text-display font-semibold tabular-nums font-mono tracking-tight truncate"
        style={accent ? { color: accent } : undefined}
      >
        {value}
      </div>
      <div class="text-label text-text-3 mt-0.5">{label}</div>
    </div>
  );
}

/** 分区标题。 */
export function SectionTitle({ children, hint }: { children: ComponentChildren; hint?: string }) {
  return (
    <div class="flex items-baseline justify-between mb-3">
      <h2 class="text-label font-medium text-text-2">{children}</h2>
      {hint && <span class="text-micro text-text-4 font-mono">{hint}</span>}
    </div>
  );
}
