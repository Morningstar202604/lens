interface Slice {
  label: string;
  value: number;
  color: string;
}

interface Props {
  data: Slice[];
  size?: number;
}

export function DonutChart({ data, size = 100 }: Props) {
  const total = data.reduce((s, d) => s + d.value, 0);
  if (total === 0) {
    return (
      <div class="flex items-center justify-center text-text-4 text-sm" style={{ width: size, height: size }}>
        暂无数据
      </div>
    );
  }

  const cx = size / 2;
  const cy = size / 2;
  const r = size / 2 - 4;
  const strokeWidth = 10;
  const circumference = 2 * Math.PI * r;

  let offset = 0;
  const slices = data.filter((d) => d.value > 0);

  return (
    <div class="flex items-center gap-4">
      <svg width={size} height={size} class="flex-shrink-0">
        {slices.map((slice) => {
          const pct = slice.value / total;
          const dashLen = circumference * pct;
          const dashGap = circumference - dashLen;
          const currentOffset = offset;
          offset += dashLen;
          return (
            <circle
              cx={cx}
              cy={cy}
              r={r}
              fill="none"
              stroke={slice.color}
              strokeWidth={strokeWidth}
              strokeDasharray={`${dashLen} ${dashGap}`}
              strokeDashoffset={-currentOffset}
              transform={`rotate(-90 ${cx} ${cy})`}
              class="transition-all duration-500"
            />
          );
        })}
      </svg>
      <div class="flex flex-col gap-1.5 flex-1 min-w-0">
        {slices.map((slice) => (
          <div key={slice.label} class="flex items-center gap-2 text-sm">
            <span class="w-2 h-2 rounded-full flex-shrink-0" style={{ background: slice.color }} />
            <span class="text-text-2 truncate flex-1">{slice.label}</span>
            <span class="text-text-4 font-semibold tabular-nums">
              {((slice.value / total) * 100).toFixed(0)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
