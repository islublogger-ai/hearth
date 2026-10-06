import MarkdownIt from 'markdown-it';
const markdown = new MarkdownIt({ html: false, linkify: true, breaks: true, typographer: false });
const escape = (text: string) => markdown.utils.escapeHtml(text);
markdown.renderer.rules.fence = (tokens, index) => {
  const token = tokens[index]; const language = token.info.trim().split(/\s+/)[0] || 'code';
  return `<div class="code-block"><div class="code-header"><span>${escape(language)}</span><button type="button" class="copy-code" aria-label="Copy code">Copy</button></div><pre><code>${escape(token.content)}</code></pre></div>`;
};
// Model-supplied images must not make silent external network requests.
markdown.renderer.rules.image = (tokens, index) => `<span class="image-placeholder">[Image: ${escape(tokens[index].content || 'not loaded')}]</span>`;
const defaultLink = markdown.renderer.rules.link_open;
markdown.renderer.rules.link_open = (tokens, index, options, env, self) => {
  tokens[index].attrSet('target', '_blank'); tokens[index].attrSet('rel', 'noopener noreferrer');
  return defaultLink ? defaultLink(tokens, index, options, env, self) : self.renderToken(tokens, index, options);
};
export function renderMarkdown(text: string): string { return markdown.render(text); }
