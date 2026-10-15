/** 展示层格式化：数字、时间、路径。
 *
 * 这些是"给人看"的转换，与接口契约无关，故不放进 generated/。
 */

/** 大数缩写。42 亿 tokens 直接印出来会撑爆版面且读不出量级。 */
export function compactNumber(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  const abs = Math.abs(n);
  if (abs < 1000) return String(n);
  if (abs < 10_000) return n.toLocaleString('en-US');
  if (abs < 1_000_000) return `${(n / 1000).toFixed(abs < 100_000 ? 1 : 0)}K`;
  if (abs < 1_000_000_000) return `${(n / 1_000_000).toFixed(abs < 10_000_000 ? 1 : 0)}M`;
  return `${(n / 1_000_000_000).toFixed(1)}B`;
}

/** 精确数字（用于 hover 提示，保留完整信息）。 */
export function exactNumber(n: number | null | undefined): string {
  return n == null ? '—' : n.toLocaleString('en-US');
}

/** 相对时间。会话列表里"3 天前"比"2026-10-06T18:00:00"有用得多。 */
export function relativeTime(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return '';
  // 后端可能给 "2026-10-06T18:00:00"（无时区，视为本地）或带 Z/偏移
  const t = parseTimestamp(iso);
  if (t == null) return '';
  const diffSec = Math.round((now - t) / 1000);
  if (diffSec < 0) return '刚刚';
  if (diffSec < 60) return '刚刚';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} 小时前`;
  if (diffSec < 86400 * 30) return `${Math.floor(diffSec / 86400)} 天前`;
  if (diffSec < 86400 * 365) return `${Math.floor(diffSec / (86400 * 30))} 个月前`;
  return `${Math.floor(diffSec / (86400 * 365))} 年前`;
}

function parseTimestamp(iso: string): number | null {
  const hasZone = /(Z|[+-]\d{2}:?\d{2})$/.test(iso);
  const t = Date.parse(hasZone ? iso : `${iso}Z`);
  return Number.isNaN(t) ? null : t;
}

/** 绝对时间（hover 用），本地时区，秒级。 */
export function absoluteTime(iso: string | null | undefined): string {
  if (!iso) return '';
  const t = parseTimestamp(iso);
  if (t == null) return iso;
  const d = new Date(t);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 时分秒（时间线气泡）。 */
export function clockTime(iso: string | null | undefined): string {
  if (!iso) return '';
  const t = parseTimestamp(iso);
  if (t == null) return iso.slice(11, 19);
  const d = new Date(t);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 路径缩短：只留末尾两段，前缀用 … 省略。 */
export function shortenPath(p: string, keep = 2): string {
  const parts = p.split(/[/\\]/).filter(Boolean);
  if (parts.length <= keep) return p;
  return `…/${parts.slice(-keep).join('/')}`;
}

/** 百分比。 */
export function percent(value: number, total: number): string {
  if (!total) return '0%';
  const p = (value / total) * 100;
  return p < 1 && p > 0 ? '<1%' : `${Math.round(p)}%`;
}
