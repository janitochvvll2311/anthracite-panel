/**
 * CodeMirror wiring.
 *
 * Loaded with a dynamic `import()` from the editor component, so the initial
 * bundle does not carry a grammar per language. Everything CodeMirror-specific
 * stays here: the component only sees the small `EditorHandle` surface, which
 * is also what the component tests inject.
 */

import { EditorView, basicSetup } from 'codemirror'
import { EditorState } from '@codemirror/state'
import { keymap } from '@codemirror/view'
import { indentWithTab } from '@codemirror/commands'
import type { Extension } from '@codemirror/state'
import type { LanguageId } from './language'

export interface EditorHandle {
  getValue: () => string
  focus: () => void
  lineCount: () => number
  destroy: () => void
}

/**
 * Builds the highlighting extension for one language.
 *
 * Each grammar is imported lazily and separately, so opening a Markdown file
 * does not download the Rust parser.
 */
async function languageExtension(id: LanguageId): Promise<Extension> {
  switch (id) {
    case 'javascript': {
      const { javascript } = await import('@codemirror/lang-javascript')
      return javascript({ typescript: true, jsx: true })
    }
    case 'json': {
      const { json } = await import('@codemirror/lang-json')
      return json()
    }
    case 'python': {
      const { python } = await import('@codemirror/lang-python')
      return python()
    }
    case 'markdown': {
      const { markdown } = await import('@codemirror/lang-markdown')
      return markdown()
    }
    case 'rust': {
      const { rust } = await import('@codemirror/lang-rust')
      return rust()
    }
    case 'html': {
      const { html } = await import('@codemirror/lang-html')
      return html()
    }
    case 'css': {
      const { css } = await import('@codemirror/lang-css')
      return css()
    }
    case 'yaml': {
      const { yaml } = await import('@codemirror/lang-yaml')
      return yaml()
    }
    default:
      // `shell`, `toml` and `plain` have no grammar in the dependency set, so
      // they keep line numbers, bracket matching and autocomplete of words
      // rather than pretending to highlight.
      return []
  }
}

export async function createEditor(
  parent: HTMLElement,
  doc: string,
  language: LanguageId,
  onChange: () => void,
): Promise<EditorHandle> {
  const extension = await languageExtension(language)
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        basicSetup,
        keymap.of([indentWithTab]),
        extension,
        EditorView.theme({
          '&': { height: '100%', fontSize: '0.8rem' },
          '.cm-scroller': {
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
            lineHeight: '1.5',
          },
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChange()
          }
        }),
      ],
    }),
  })

  return {
    getValue: () => view.state.doc.toString(),
    focus: () => view.focus(),
    lineCount: () => view.state.doc.lines,
    destroy: () => view.destroy(),
  }
}
