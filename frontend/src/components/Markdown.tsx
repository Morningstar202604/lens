import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

export function renderMarkdown(src: string): string {
  const rawHtml = marked.parse(src) as string;
  return DOMPurify.sanitize(rawHtml);
}

export function Markdown({ source }: { source: string }) {
  if (!source) return null;
  const html = renderMarkdown(source);
  return (
    <div
      class="prose prose-invert prose-sm max-w-none prose-headings:text-gray-200 prose-p:text-gray-300 prose-strong:text-white prose-code:text-amber-300 prose-pre:bg-gray-800/80 prose-pre:border prose-pre:border-gray-700 prose-blockquote:border-gray-600"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
