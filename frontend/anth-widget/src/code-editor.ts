import { msg } from '@lit/localize'

import type { EditorState, Extension } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'

export interface CodeEditorCallbacks {
  onChange(text: string): void
  onCursor(line: number, column: number, selected: number): void
  onLanguage(name: string): void
}

export interface CodeEditorOptions {
  doc: string
  /** Filename used to pick the language. */
  name: string
  ariaLabel: string
  callbacks: CodeEditorCallbacks
}

export interface CodeEditor {
  focus(): void
  setText(text: string): void
  setLanguage(name: string): void
  destroy(): void
}

/** The test DOM lacks the selection APIs CodeMirror needs. */
const inTests =
  (import.meta as ImportMeta & { env?: { MODE?: string } }).env?.MODE === 'test'

/**
 * Loads CodeMirror and its language data on demand. Returning `null` leaves the
 * caller on the plain textarea fallback.
 */
export async function createCodeEditor(
  parent: HTMLElement,
  options: CodeEditorOptions,
): Promise<CodeEditor | null> {
  if (inTests) {
    return null
  }

  const [
    { EditorView, basicSetup },
    { Compartment, EditorState },
    { HighlightStyle, LanguageDescription, syntaxHighlighting },
    { tags },
    { languages },
  ] = await Promise.all([
    import('codemirror'),
    import('@codemirror/state'),
    import('@codemirror/language'),
    import('@lezer/highlight'),
    import('@codemirror/language-data'),
  ])

  const highlight = HighlightStyle.define([
    { tag: tags.keyword, color: '#c792ea' },
    {
      tag: [tags.name, tags.deleted, tags.character, tags.propertyName, tags.macroName],
      color: '#dbe6f2',
    },
    { tag: [tags.function(tags.variableName), tags.labelName], color: '#7dd3fc' },
    {
      tag: [tags.color, tags.constant(tags.name), tags.standard(tags.name)],
      color: '#fbbf24',
    },
    { tag: [tags.definition(tags.name), tags.separator], color: '#dbe6f2' },
    {
      tag: [
        tags.typeName,
        tags.className,
        tags.number,
        tags.changed,
        tags.annotation,
        tags.modifier,
        tags.self,
        tags.namespace,
      ],
      color: '#f59e0b',
    },
    {
      tag: [tags.operator, tags.operatorKeyword, tags.url, tags.escape, tags.regexp],
      color: '#7dd3fc',
    },
    { tag: [tags.meta, tags.comment], color: '#8b9bad', fontStyle: 'italic' },
    {
      tag: [
        tags.heading,
        tags.heading1,
        tags.heading2,
        tags.heading3,
        tags.heading4,
        tags.heading5,
        tags.heading6,
      ],
      color: '#7dd3fc',
      fontWeight: 'bold',
    },
    { tag: tags.quote, color: '#8b9bad', fontStyle: 'italic' },
    { tag: [tags.monospace, tags.processingInstruction], color: '#fbbf24' },
    { tag: tags.contentSeparator, color: '#8b9bad' },
    { tag: tags.strong, fontWeight: 'bold' },
    { tag: tags.emphasis, fontStyle: 'italic' },
    { tag: tags.strikethrough, textDecoration: 'line-through' },
    { tag: tags.link, color: '#7dd3fc', textDecoration: 'underline' },
    { tag: [tags.atom, tags.bool, tags.special(tags.variableName)], color: '#f87171' },
    { tag: [tags.processingInstruction, tags.string, tags.inserted], color: '#86efac' },
    { tag: tags.invalid, color: '#f87171' },
  ])

  const language = new Compartment()

  const detect = async (name: string): Promise<{ support: Extension | null; name: string }> => {
    const description = LanguageDescription.matchFilename(languages, name)
    if (!description) {
      return { support: null, name: msg('Plain text') }
    }
    try {
      return { support: await description.load(), name: description.name }
    } catch {
      return { support: null, name: msg('Plain text') }
    }
  }

  const initial = await detect(options.name)
  options.callbacks.onLanguage(initial.name)

  const reportCursor = (state: EditorState): void => {
    const head = state.selection.main.head
    const line = state.doc.lineAt(head)
    options.callbacks.onCursor(
      line.number,
      head - line.from + 1,
      Math.abs(state.selection.main.to - state.selection.main.from),
    )
  }

  const state = EditorState.create({
    doc: options.doc,
    extensions: [
      basicSetup,
      language.of(initial.support ?? []),
      syntaxHighlighting(highlight),
      EditorView.contentAttributes.of({
        'aria-label': options.ariaLabel,
        spellcheck: 'false',
      }),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          options.callbacks.onChange(update.state.doc.toString())
        }
        if (update.docChanged || update.selectionSet) {
          reportCursor(update.state)
        }
      }),
      EditorView.theme(
        {
          '&': {
            height: '100%',
            color: 'var(--text, #dbe6f2)',
            backgroundColor: 'transparent',
            fontSize: '12px',
          },
          '.cm-scroller': {
            fontFamily: 'var(--mono, monospace)',
            lineHeight: '1.5',
            overflow: 'auto',
          },
          '.cm-content': { caretColor: 'var(--accent, #7dd3fc)' },
          '.cm-gutters': {
            backgroundColor: 'transparent',
            color: 'var(--muted, #8b9bad)',
            border: 'none',
          },
          '.cm-activeLine': { backgroundColor: 'rgba(125, 211, 252, 0.06)' },
          '.cm-activeLineGutter': { backgroundColor: 'transparent' },
          '&.cm-focused': { outline: 'none' },
          '.cm-selectionBackground': { backgroundColor: 'rgba(125, 211, 252, 0.25)' },
          '&.cm-focused .cm-selectionBackground': {
            backgroundColor: 'rgba(125, 211, 252, 0.3)',
          },
          '.cm-cursor': { borderLeftColor: 'var(--accent, #7dd3fc)' },
        },
        { dark: true },
      ),
    ],
  })

  const view: EditorView = new EditorView({ state, parent })
  reportCursor(state)

  let alive = true
  return {
    focus() {
      view.requestMeasure()
      view.focus()
    },
    setText(text) {
      const current = view.state.doc.toString()
      if (current !== text) {
        view.dispatch({ changes: { from: 0, to: current.length, insert: text } })
      }
    },
    setLanguage(name) {
      void detect(name).then((detected) => {
        if (!alive) {
          return
        }
        options.callbacks.onLanguage(detected.name)
        view.dispatch({
          effects: language.reconfigure(detected.support ?? []),
        })
      })
    },
    destroy() {
      alive = false
      view.destroy()
    },
  }
}
