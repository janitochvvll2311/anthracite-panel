/**
 * Language selection for the editor.
 *
 * Kept separate from the component so the mapping from a file name to a
 * language can be tested without mounting CodeMirror.
 */

export type LanguageId =
  | 'javascript'
  | 'json'
  | 'python'
  | 'markdown'
  | 'rust'
  | 'html'
  | 'css'
  | 'yaml'
  | 'shell'
  | 'toml'
  | 'plain'

interface LanguageRule {
  id: LanguageId
  names?: string[]
  extensions?: string[]
}

/**
 * Only extensions with a real grammar are listed; everything else falls back to
 * `plain`, which still gets line numbers and bracket matching.
 */
const RULES: LanguageRule[] = [
  { id: 'javascript', extensions: ['.js', '.mjs', '.cjs', '.jsx', '.ts', '.tsx'] },
  { id: 'json', names: ['package.json', 'tsconfig.json'], extensions: ['.json', '.jsonc'] },
  { id: 'python', extensions: ['.py', '.pyi'] },
  { id: 'markdown', extensions: ['.md', '.markdown'] },
  { id: 'rust', extensions: ['.rs'] },
  { id: 'html', extensions: ['.html', '.htm', '.xhtml'] },
  { id: 'css', extensions: ['.css', '.scss', '.less'] },
  { id: 'yaml', extensions: ['.yml', '.yaml'] },
  {
    id: 'shell',
    names: ['.bashrc', '.profile', '.zshrc'],
    extensions: ['.sh', '.bash', '.zsh'],
  },
  { id: 'toml', names: ['.editorconfig'], extensions: ['.toml', '.gitignore'] },
]

const BINARY_EXTENSIONS = [
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.bmp',
  '.ico',
  '.pdf',
  '.zip',
  '.gz',
  '.xz',
  '.7z',
  '.tar',
  '.woff',
  '.woff2',
  '.ttf',
  '.otf',
  '.mp3',
  '.mp4',
  '.so',
  '.a',
  '.o',
  '.wasm',
]

/** The language for one path, derived from its base name. */
export function languageFor(path: string): LanguageId {
  const base = (path.split('/').pop() ?? '').toLowerCase()
  const named = RULES.find((rule) => rule.names?.includes(base))
  if (named) {
    return named.id
  }
  // Longest extension wins, so a compound suffix cannot be shadowed by a
  // shorter one.
  const rules = RULES.flatMap((rule) =>
    (rule.extensions ?? []).map((extension) => ({ id: rule.id, extension })),
  ).sort((a, b) => b.extension.length - a.extension.length)
  const match = rules.find((rule) => base.endsWith(rule.extension))
  return match?.id ?? 'plain'
}

/**
 * Whether the panel should offer to edit this path.
 *
 * Only known-binary extensions are refused, so `README`, `Makefile` and other
 * extensionless text files stay editable.
 */
export function isEditable(path: string): boolean {
  const lower = path.toLowerCase()
  return !BINARY_EXTENSIONS.some((extension) => lower.endsWith(extension))
}
