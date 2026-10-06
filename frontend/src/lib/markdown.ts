import DOMPurify from 'dompurify'
// The `common` bundle covers ~40 languages and keeps the vendor chunk small.
import hljs from 'highlight.js/lib/common'
import { Marked } from 'marked'

/**
 * Render user-supplied Markdown to sanitized HTML.
 *
 * READMEs and wiki pages come from GitHub submitters, so the output is passed
 * through DOMPurify before it reaches `v-html`: `marked` does not sanitise and
 * a submitted README is untrusted input.
 */
const marked = new Marked({
  gfm: true,
  breaks: false,
})

marked.use({
  renderer: {
    code(token: { text: string; lang?: string }) {
      const language = token.lang?.split(/\s+/)[0]
      // getLanguage returns the Language object, so keep the name separately.
      const known = language !== undefined && hljs.getLanguage(language) !== undefined
      const html = known
        ? hljs.highlight(token.text, { language: language as string }).value
        : escapeHtml(token.text)
      const cls = known ? ` class="hljs language-${language}"` : ' class="hljs"'
      return `<pre><code${cls}>${html}</code></pre>`
    },
  },
})

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

export function renderMarkdown(source: string | null | undefined): string {
  if (!source) return ''
  const raw = marked.parse(source, { async: false }) as string
  return DOMPurify.sanitize(raw, {
    ADD_ATTR: ['target', 'rel'],
    FORBID_ATTR: ['style'],
    FORBID_TAGS: ['style', 'form', 'input', 'button'],
  })
}

// All links open in a new tab and cannot reach back via `window.opener`.
// This is a DOMPurify hook — the previous regex-based hardening operated on
// the *sanitized* string and was exploitable via attribute-value injection
// (`<img src="broken" alt="<a onerror=...>">`) .
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})