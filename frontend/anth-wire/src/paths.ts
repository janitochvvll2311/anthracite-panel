/** A display path: always starts at `/` and has no empty or `.` segments. */
export function normalize(path: string): string {
  const parts = path.split('/').filter((part) => part !== '' && part !== '.')
  return `/${parts.join('/')}`
}

export function join(base: string, name: string): string {
  return normalize(`${base}/${name}`)
}

export function parent(path: string): string {
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return `/${parts.join('/')}`
}

/** The final segment of a display path. */
export function baseName(path: string): string {
  const parts = path.split('/').filter(Boolean)
  return parts.at(-1) ?? path
}
