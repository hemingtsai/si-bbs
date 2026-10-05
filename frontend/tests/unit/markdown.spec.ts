import { describe, expect, it } from 'vitest'

import { hardenLinks, renderMarkdown } from '../../src/lib/markdown'

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

describe('hardenLinks', () => {
  it('adds target and rel to anchors', () => {
    const out = hardenLinks('<a href="https://example.test">x</a>')
    expect(out).toContain('target="_blank"')
    expect(out).toContain('rel="noopener noreferrer"')
  })

  it('leaves markup without anchors untouched', () => {
    expect(hardenLinks('<p>hello</p>')).toBe('<p>hello</p>')
  })
})