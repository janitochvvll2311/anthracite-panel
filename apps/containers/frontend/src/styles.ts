export const styles = `
  .cn { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .cn-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .cn-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .cn button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn button:hover { border-color: var(--accent); }
  .cn button.primary { border-color: var(--accent); background: var(--panel-3); }
  .cn button.danger { border-color: var(--danger); color: var(--danger); }
  .cn button:disabled { opacity: .55; cursor: default; }
  .cn input[type="text"], .cn input[type="search"], .cn select {
    padding: 5px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px;
  }
  .cn-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .cn-status.bad { color: var(--danger); }
  .cn-tabs { display: flex; gap: 4px; flex-wrap: wrap; }
  .cn-tab {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--muted); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn-tab.active { border-color: var(--accent); background: var(--panel-2); color: var(--text); }
  .cn-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(290px, 1fr)); gap: 10px; }
  .cn-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
    cursor: pointer;
  }
  .cn-card:hover { border-color: var(--line); }
  .cn-card.selected { border-color: var(--accent); }
  .cn-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .cn-card-head strong {
    font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cn-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .cn-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .cn-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .cn-badge.ok { border-color: var(--good); color: var(--good); }
  .cn-badge.warn { border-color: var(--warn); color: var(--warn); }
  .cn-badge.bad { border-color: var(--danger); color: var(--danger); }
  .cn-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .cn-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .cn-note { font-size: 12px; color: var(--muted); }
  .cn-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .cn-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-detail-head strong { font-size: 13px; color: var(--text); }
  .cn-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .cn-facts { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }
  .cn-strip { display: flex; gap: 10px; flex-wrap: wrap; }
  .cn-strip span { font-size: 12px; color: var(--muted); }
  .cn-strip strong { color: var(--text); font-weight: 600; }
  .cn-log {
    margin: 0; padding: 10px; border-radius: 8px; border: 1px solid var(--line-soft);
    background: var(--bg); color: var(--text); font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; line-height: 1.45; max-height: 320px; overflow: auto; white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cn-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(560px, calc(100vw - 32px));
  }
  .cn-dialog::backdrop { background: #05070bb3; }
  .cn-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .cn-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .cn-dialog .cn-actions { justify-content: flex-end; margin-top: 12px; }
  .cn-exec-out {
    margin: 0 0 8px; padding: 8px; min-height: 180px; max-height: 300px; overflow: auto;
    border-radius: 8px; border: 1px solid var(--line-soft); background: var(--bg);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .cn-exec-line { display: flex; gap: 6px; }
  .cn-exec-line input { flex: 1; }

  html[data-viewport="tablet"] .cn-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="mobile"] .cn-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .cn-actions { width: 100%; }
  html[data-viewport="mobile"] .cn-actions button { flex: 1 1 auto; }

  @container (max-width: 640px) {
    .cn-grid { grid-template-columns: minmax(0, 1fr); }
    .cn-actions button { flex: 1 1 auto; }
  }
`;
