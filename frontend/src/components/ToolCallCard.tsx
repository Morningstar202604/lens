import { useState } from 'preact/hooks';
import hljs from 'highlight.js/lib/core';
import json from 'highlight.js/lib/languages/json';
import 'highlight.js/styles/github-dark.css';

hljs.registerLanguage('json', json);

interface Props {
  call: { name: string; arguments: Record<string, unknown> };
}

/** 一次工具调用。参数里的长字符串（文件内容、代码）先摘要再展开，避免刷屏。 */
export function ToolCallCard({ call }: Props) {
  const [open, setOpen] = useState(false);
  const entries = Object.entries(call.arguments ?? {});
  const preview = summarize(entries);

  return (
    <div class="mb-1.5 border border-border-dim rounded-md overflow-hidden">
      <button
        onClick={() => setOpen(!open)}
        class="w-full flex items-center gap-2 px-2.5 py-1.5 text-left hover:bg-bg-card transition-colors"
      >
        <span class={`text-micro text-text-4 transition-transform ${open ? 'rotate-90' : ''}`}>▶</span>
        <span class="text-note font-mono text-primary/85">{call.name}</span>
        {preview && (
          <span class="text-micro font-mono text-text-4 truncate flex-1 min-w-0">{preview}</span>
        )}
      </button>
      {open && (
        <pre class="px-2.5 py-2 font-mono text-micro leading-relaxed bg-bg border-t border-border-dim text-text-2 whitespace-pre-wrap max-h-[320px] overflow-y-auto scroll-thin">
          <code
            class="hljs bg-transparent p-0"
            dangerouslySetInnerHTML={{ __html: hljs.highlight(JSON.stringify(call.arguments, null, 2), { language: 'json' }).value }}
          />
        </pre>
      )}
    </div>
  );
}

/** 折叠态下显示最关键的一两个参数值。 */
function summarize(entries: [string, unknown][]): string {
  const keys = ['filePath', 'file_path', 'path', 'command', 'pattern', 'query', 'url'];
  for (const k of keys) {
    const hit = entries.find(([name]) => name === k);
    if (hit && typeof hit[1] === 'string') {
      const v = hit[1] as string;
      return v.length > 70 ? `${v.slice(0, 70)}…` : v;
    }
  }
  if (entries.length === 0) return '';
  const [k, v] = entries[0];
  const s = typeof v === 'string' ? v : JSON.stringify(v);
  return `${k}: ${s && s.length > 46 ? `${s.slice(0, 46)}…` : s}`;
}
