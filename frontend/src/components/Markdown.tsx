import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

export function renderMarkdown(src: string): string {
  const rawHtml = marked.parse(src) as string;
  // 内容来自被拦截的 agent 流量，属不可信输入 —— 必须过 DOMPurify
  return DOMPurify.sanitize(rawHtml);
}

export function Markdown({ source }: { source: string }) {
  if (!source) return null;
  const html = renderMarkdown(source);
  return <div class="prose-lens" dangerouslySetInnerHTML={{ __html: html }} />;
}
