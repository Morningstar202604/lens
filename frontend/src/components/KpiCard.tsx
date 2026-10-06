interface Props {
  label: string;
  value: string | number;
  color?: string;
}

export function KpiCard({ label, value, color = '#6366f5' }: Props) {
  return (
    <div class="bg-bg-card rounded-lg p-3 border border-border hover:border-border-hover transition-colors">
      <div class="text-xl font-bold tabular-nums" style={{ color }}>
        {typeof value === 'number' ? value.toLocaleString() : value}
      </div>
      <div class="text-xs text-text-4 mt-0.5">{label}</div>
    </div>
  );
}
