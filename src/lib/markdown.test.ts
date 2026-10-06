import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './markdown';
describe('model-output rendering', () => {
  it('never executes raw HTML or javascript links', () => {
    const result = renderMarkdown('<script>bad()</script>\n[x](javascript:bad())\n<img src=x onerror=bad()>');
    expect(result).not.toContain('<script>'); expect(result).not.toContain('<img'); expect(result).not.toContain('href="javascript:');
  });
  it('escapes code and code language attributes while providing a copy action', () => {
    const result = renderMarkdown('```html\n<img src=x>\n```');
    expect(result).toContain('&lt;img'); expect(result).toContain('Copy code'); expect(result).not.toContain('<img');
  });
  it('isolates external links from the app', () => {
    const result = renderMarkdown('[docs](https://example.com)');
    expect(result).toContain('noopener noreferrer'); expect(result).toContain('target="_blank"');
  });
});
