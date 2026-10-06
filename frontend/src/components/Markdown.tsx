import { useMemo } from 'preact/hooks';
import { marked } from 'marked';
import hljs from 'highlight.js/lib/core';
import javascript from 'highlight.js/lib/languages/javascript';
import typescript from 'highlight.js/lib/languages/typescript';
import python from 'highlight.js/lib/languages/python';
import json from 'highlight.js/lib/languages/json';
import bash from 'highlight.js/lib/languages/bash';
import rust from 'highlight.js/lib/languages/rust';
import 'highlight.js/styles/github-dark.css';

// Register only the languages we need
hljs.registerLanguage('javascript', javascript);
hljs.registerLanguage('typescript', typescript);
hljs.registerLanguage('python', python);
hljs.registerLanguage('json', json);
hljs.registerLanguage('bash', bash);
hljs.registerLanguage('rust', rust);

// Configure marked once
marked.setOptions({ gfm: true, breaks: true });

const renderer = new marked.Renderer();
renderer.code = function ({ text, lang }: { text: string; lang?: string }) {
  const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext';
  const highlighted = hljs.highlight(text, { language }).value;
  return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`;
};

marked.use({ renderer });

export function renderMarkdown(src: string): string {
  return marked.parse(src) as string;
}

interface Props {
  content: string;
}

export function Markdown({ content }: Props) {
  const html = useMemo(() => renderMarkdown(content), [content]);

  return (
    <div
      class="prose prose-invert prose-sm max-w-none leading-relaxed
             [&_pre]:bg-bg [&_pre]:border [&_pre]:border-border-dim [&_pre]:border-l-primary
             [&_pre]:rounded-md [&_pre]:p-3 [&_pre]:overflow-x-auto
             [&_code]:font-mono [&_code]:text-xs
             [&_:not(pre)_>code]:bg-white/5 [&_:not(pre)_>code]:px-1.5 [&_:not(pre)_>code]:py-0.5
             [&_:not(pre)_>code]:rounded"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
