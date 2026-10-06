import { describe, expect, it } from 'vitest'

import { renderMarkdown } from '../../src/lib/markdown'

describe('renderMarkdown', () => {
  it('renders headings and emphasis', () => {
    const html = renderMarkdown('# Title\n\nSome **bold** text.')
    expect(html).toContain('<h1')
    expect(html).toContain('<strong>bold</strong>')
  })

  it('returns an empty string for empty input', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown(null)).toBe('')
    expect(renderMarkdown(undefined)).toBe('')
  })

  it('strips script tags from untrusted READMEs', () => {
    const html = renderMarkdown('Hello <script>alert(1)</script> world')
    expect(html).not.toContain('<script')
    expect(html).not.toContain('alert(1)')
    expect(html).toContain('Hello')
  })

  it('strips inline event handlers and javascript: urls', () => {
    const html = renderMarkdown('<img src=x onerror="alert(1)">')
    expect(html).not.toContain('onerror')

    const link = renderMarkdown('[click](javascript:alert(1))')
    expect(link).not.toContain('javascript:')
  })

  it('strips iframes and object embeds', () => {
    const html = renderMarkdown('<iframe src="https://evil.test"></iframe>')
    expect(html).not.toContain('<iframe')
  })

  it('highlights fenced code with a known language', () => {
    const html = renderMarkdown('```rust\nfn main() {}\n```')
    expect(html).toContain('hljs')
    expect(html).toContain('language-rust')
  })

  it('escapes code blocks for unknown languages instead of injecting', () => {
    const html = renderMarkdown('```notalanguage\n<script>alert(1)</script>\n```')
    expect(html).not.toContain('<script')
    expect(html).toContain('&lt;script&gt;')
  })

  it('renders GitHub tables and lists', () => {
    const table = renderMarkdown('| a | b |\n| - | - |\n| 1 | 2 |')
    expect(table).toContain('<table>')

    const list = renderMarkdown('- one\n- two')
    expect(list).toContain('<ul>')
  })
})

describe('DOMPurify afterSanitizeAttributes hook', () => {
  it('adds target and rel to anchors produced from Markdown links', () => {
    const html = renderMarkdown('[click](https://example.test)')
    expect(html).toContain('target="_blank"')
    expect(html).toContain('rel="noopener noreferrer"')
  })

  it('strips style attributes (tracking beacon / UI spoofing)', () => {
    const html = renderMarkdown('<p style="position:fixed;inset:0;z-index:99999">x</p>')
    expect(html).not.toContain('style=')
  })

  it('does not leave exploitable attribute injection (regression)', () => {
    const html = renderMarkdown('<img src="broken" alt="<a onerror=alert(1)>">')
    // The alt text may still literally contain "onerror" — it must NOT become
    // a live event handler when the browser parses the fragment.
    const doc = new DOMParser().parseFromString(html, 'text/html')
    expect(doc.querySelector('img')!.getAttribute('onerror')).toBeNull()
    expect(html).not.toContain('<a target="_blank" rel="noopener noreferrer" onerror=')
  })
})
