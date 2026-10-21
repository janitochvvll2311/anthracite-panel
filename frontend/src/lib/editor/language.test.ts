import { describe, expect, it } from 'vitest'
import { isEditable, languageFor } from './language'

describe('languageFor', () => {
  it('maps common extensions', () => {
    expect(languageFor('/a/b/main.rs')).toBe('rust')
    expect(languageFor('/x/app.ts')).toBe('javascript')
    expect(languageFor('/x/app.mjs')).toBe('javascript')
    expect(languageFor('/x/settings.json')).toBe('json')
    expect(languageFor('/x/script.py')).toBe('python')
    expect(languageFor('/x/notes.md')).toBe('markdown')
    expect(languageFor('/x/index.html')).toBe('html')
    expect(languageFor('/x/site.css')).toBe('css')
    expect(languageFor('/x/compose.yml')).toBe('yaml')
    expect(languageFor('/x/run.sh')).toBe('shell')
    expect(languageFor('/x/Cargo.toml')).toBe('toml')
  })

  it('is case insensitive', () => {
    expect(languageFor('/x/Main.RS')).toBe('rust')
    expect(languageFor('/x/README.MD')).toBe('markdown')
  })

  it('prefers a whole-name rule over an extension', () => {
    // `package.json` is JSON, but the rule exists so it can diverge later
    // without every `.json` file changing too.
    expect(languageFor('/repo/package.json')).toBe('json')
    expect(languageFor('/home/u/.bashrc')).toBe('shell')
  })

  it('falls back to plain for unknown and extensionless names', () => {
    expect(languageFor('/x/file.unknownext')).toBe('plain')
    expect(languageFor('/x/Makefile')).toBe('plain')
    expect(languageFor('/x/README')).toBe('plain')
  })

  it('handles a bare file name with no directory', () => {
    expect(languageFor('main.rs')).toBe('rust')
  })

  it('does not treat a directory name as an extension', () => {
    // `src/rust` must not be read as a `.rust` file.
    expect(languageFor('/src/rust')).toBe('plain')
  })
})

describe('isEditable', () => {
  it('accepts text formats and extensionless files', () => {
    expect(isEditable('/etc/nginx/nginx.conf')).toBe(true)
    expect(isEditable('/x/README')).toBe(true)
    expect(isEditable('/x/Makefile')).toBe(true)
    expect(isEditable('/x/.env')).toBe(true)
  })

  it('refuses known binary formats', () => {
    expect(isEditable('/x/photo.png')).toBe(false)
    expect(isEditable('/x/archive.zip')).toBe(false)
    expect(isEditable('/x/lib.so')).toBe(false)
    expect(isEditable('/x/doc.pdf')).toBe(false)
  })

  it('is case insensitive', () => {
    expect(isEditable('/x/PHOTO.JPG')).toBe(false)
  })
})
