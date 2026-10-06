import { useState } from 'preact/hooks';
import hljs from 'highlight.js/lib/core';
import json from 'highlight.js/lib/languages/json';
import 'highlight.js/styles/github-dark.css';

hljs.registerLanguage('json', json);

interface Props {
  call: { name: string; arguments: Record<string, unknown> };
}

export function ToolCallCard({ call }: Props) {
  const [open, setOpen] = useState(false);
  const argsJson = JSON.stringify(call.arguments, null, 2);
  const highlighted = hljs.highlight(argsJson, { language: 'json' }).value;

  return (
    <div class={`border border-border rounded-md overflow-hidden my-1.5 transition-colors ${open ? 'border-border-hover' : 'hover:border-border-hover'}`}>
      <button
        onClick={() => setOpen(!open)}
        class="flex items-center gap-1.5 px-3 py-1.5 bg-bg-elev w-full text-left text-sm font-medium text-primary cursor-pointer"
      >
        <span class={`text-[8px] transition-transform ${open ? 'rotate-90' : ''}`}>▶</span>
        <span class="font-mono">{call.name}</span>
      </button>
      {open && (
        <pre class="p-3.5 font-mono text-xs leading-relaxed bg-bg text-text-2 whitespace-pre-wrap max-h-[200px] overflow-y-auto scroll-thin">
          <code class="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
        </pre>
      )}
    </div>
  );
}
