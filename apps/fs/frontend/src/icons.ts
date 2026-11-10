import { extensionOf } from "./format";
import { escape } from "./util";

export const IMAGES: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  bmp: "image/bmp",
  ico: "image/x-icon",
  svg: "image/svg+xml",
};

const MIME_BY_EXTENSION: Record<string, string> = {
  js: "text/javascript",
  mjs: "text/javascript",
  cjs: "text/javascript",
  jsx: "text/javascript",
  ts: "text/typescript",
  tsx: "text/typescript",
  json: "application/json",
  md: "text/markdown",
  markdown: "text/markdown",
  txt: "text/plain",
  log: "text/plain",
  html: "text/html",
  htm: "text/html",
  css: "text/css",
  scss: "text/x-scss",
  sh: "application/x-sh",
  bash: "application/x-sh",
  zsh: "application/x-sh",
  py: "text/x-python",
  rs: "text/x-rust",
  toml: "application/toml",
  yaml: "application/yaml",
  yml: "application/yaml",
  ini: "text/plain",
  ...IMAGES,
  pdf: "application/pdf",
  zip: "application/zip",
  gz: "application/gzip",
  tgz: "application/gzip",
  tar: "application/x-tar",
  "7z": "application/x-7z-compressed",
  rar: "application/vnd.rar",
  bz2: "application/x-bzip2",
  tbz2: "application/x-bzip2",
  xz: "application/x-xz",
  zst: "application/zstd",
  mp3: "audio/mpeg",
  wav: "audio/wav",
  ogg: "audio/ogg",
  flac: "audio/flac",
  mp4: "video/mp4",
  webm: "video/webm",
  mkv: "video/x-matroska",
  mov: "video/quicktime",
  wasm: "application/wasm",
  db: "application/x-sqlite3",
  sqlite: "application/vnd.sqlite3",
  sqlite3: "application/vnd.sqlite3",
  db3: "application/x-sqlite3",
  sql: "text/x-sql",
  duckdb: "application/x-duckdb",
  mdb: "application/x-msaccess",
  accdb: "application/x-msaccess",
  dbf: "application/dbase",
};

const ICON_LABELS: Record<string, string> = {
  js: "JS",
  mjs: "JS",
  cjs: "JS",
  jsx: "JSX",
  ts: "TS",
  tsx: "TSX",
  json: "JSON",
  md: "MD",
  markdown: "MD",
  txt: "TXT",
  log: "LOG",
  html: "HTML",
  htm: "HTML",
  css: "CSS",
  scss: "SCSS",
  sh: "SH",
  bash: "SH",
  zsh: "SH",
  py: "PY",
  rs: "RS",
  toml: "TOML",
  yaml: "YML",
  yml: "YML",
  ini: "INI",
  png: "PNG",
  jpg: "JPG",
  jpeg: "JPG",
  gif: "GIF",
  webp: "WEBP",
  svg: "SVG",
  avif: "AVIF",
  bmp: "BMP",
  ico: "ICO",
  pdf: "PDF",
  zip: "ZIP",
  gz: "GZ",
  tgz: "TGZ",
  tar: "TAR",
  "7z": "7Z",
  rar: "RAR",
  bz2: "BZ2",
  tbz2: "TBZ2",
  xz: "XZ",
  zst: "ZST",
  mp3: "MP3",
  wav: "WAV",
  ogg: "OGG",
  flac: "FLAC",
  mp4: "MP4",
  webm: "WEBM",
  mkv: "MKV",
  mov: "MOV",
  wasm: "WASM",
  db: "DB",
  sqlite: "SQL",
  sqlite3: "SQL",
  db3: "DB",
  sql: "SQL",
  duckdb: "DUCK",
  mdb: "MDB",
  accdb: "ACCD",
  dbf: "DBF",
};

const FOLDER_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M2.5 5.5A1.5 1.5 0 0 1 4 4h4l1.7 2h6.3A1.5 1.5 0 0 1 17.5 7.5v7A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z" fill="var(--icon-color)" fill-opacity=".18" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/></svg>';
const FILE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5 2.5h6.5L15 6v11.5H5z" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/><path d="M11.5 2.5V6H15" fill="none" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/></svg>';
const IMAGE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="3.5" width="15" height="12" rx="1.5" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><circle cx="7" cy="7.5" r="1.3" fill="var(--icon-color)"/><path d="m4.5 13.5 4-4 2.5 2.5 2-2 2.5 2.5" fill="none" stroke="var(--icon-color)" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const VIDEO_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="4" width="15" height="11.5" rx="1.5" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M8.2 7.2v5.1l4.2-2.55z" fill="var(--icon-color)"/></svg>';
const AUDIO_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M8 13.2V6.1l7-1.8v2.4L10 8v6.2" fill="none" stroke="var(--icon-color)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><circle cx="6.1" cy="14" r="1.9" fill="var(--icon-color)" fill-opacity=".22" stroke="var(--icon-color)" stroke-width="1.1"/><circle cx="13.1" cy="12.6" r="1.9" fill="var(--icon-color)" fill-opacity=".22" stroke="var(--icon-color)" stroke-width="1.1"/></svg>';
const DATABASE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><ellipse cx="10" cy="5" rx="6" ry="2.5" fill="var(--icon-color)" fill-opacity=".18" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M4 5v9.5c0 1.4 2.7 2.5 6 2.5s6-1.1 6-2.5V5" fill="none" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M4 9.5c0 1.4 2.7 2.5 6 2.5s6-1.1 6-2.5" fill="none" stroke="var(--icon-color)" stroke-width="1.1"/></svg>';
const ARCHIVE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3.5" width="14" height="13" rx="1.8" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M10 3.5v10" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M8.4 5.5h3.2M8.4 8h3.2M8.4 10.5h3.2" stroke="var(--icon-color)" stroke-width="1.05" stroke-linecap="round"/><path d="M9 13.5h2v2.2H9z" fill="var(--icon-color)"/></svg>';
const LINK_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="var(--icon-color)" stroke-width="1.3" stroke-linecap="round"><path d="m8.2 11.8 3.6-3.6"/><path d="M6.8 9.9 5.4 11.3a3 3 0 0 0 4.2 4.2l1.4-1.4"/><path d="m13.2 10.1 1.4-1.4a3 3 0 0 0-4.2-4.2L9 5.9"/></svg>';

export function mimeFor(path: string): string {
  return MIME_BY_EXTENSION[extensionOf(path)] ?? "application/octet-stream";
}

function iconLabel(path: string): string {
  const extension = extensionOf(path);
  if (!extension) return "FILE";
  return ICON_LABELS[extension] ?? extension.toUpperCase().slice(0, 4);
}

function iconCategory(mime: string, extension: string): string {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("audio/")) return "audio";
  if (mime.startsWith("video/")) return "video";
  if (mime === "application/pdf") return "pdf";
  if (
    [
      "zip",
      "gzip",
      "x-tar",
      "x-7z-compressed",
      "vnd.rar",
      "x-bzip2",
      "x-xz",
      "zstd",
    ].some((part) => mime.includes(part))
  ) {
    return "archive";
  }
  if (mime === "application/wasm") return "binary";
  if (
    [
      "db",
      "sqlite",
      "sqlite3",
      "db3",
      "sql",
      "duckdb",
      "mdb",
      "accdb",
      "dbf",
    ].includes(extension)
  ) {
    return "database";
  }
  if (
    mime === "application/json" ||
    mime.endsWith("+json") ||
    mime.includes("xml")
  ) {
    return "data";
  }
  if (["js", "mjs", "cjs", "jsx", "py", "rs"].includes(extension)) {
    return "code";
  }
  if (["ts", "tsx"].includes(extension)) return "code-strong";
  if (["md", "markdown"].includes(extension)) return "doc";
  if (["html", "htm", "css", "scss"].includes(extension)) return "web";
  if (["sh", "bash", "zsh"].includes(extension)) return "shell";
  if (["toml", "yaml", "yml", "ini"].includes(extension)) return "config";
  if (mime.startsWith("text/")) return "text";
  return "unknown";
}

export function iconMarkup(kind: string, path: string): string {
  if (kind === "dir") {
    return `<span class="file-icon dir" title="Directory">${FOLDER_ICON}</span>`;
  }
  if (kind === "link") {
    return `<span class="file-icon link" title="Symbolic link">${LINK_ICON}</span>`;
  }
  const mime = mimeFor(path);
  const extension = extensionOf(path);
  const category = iconCategory(mime, extension);
  const shape =
    category === "image"
      ? IMAGE_ICON
      : category === "video"
        ? VIDEO_ICON
        : category === "audio"
          ? AUDIO_ICON
          : category === "database"
            ? DATABASE_ICON
            : category === "archive"
              ? ARCHIVE_ICON
              : FILE_ICON;
  return `<span class="file-icon ${category}" title="${escape(mime)}">${shape}<span class="file-icon-badge">${escape(iconLabel(path))}</span></span>`;
}
