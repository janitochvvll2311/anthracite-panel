export const styles = `
  .ac { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .ac-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .ac-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .ac button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .ac button:hover { border-color: var(--accent); }
  .ac button.primary { border-color: var(--accent); background: var(--panel-3); }
  .ac button.danger { border-color: var(--danger); color: var(--danger); }
  .ac button:disabled { opacity: .55; cursor: default; }
  .ac-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .ac-status.bad { color: var(--danger); }
  .ac-section { display: flex; flex-direction: column; gap: 8px; }
  .ac-section-head { display: flex; align-items: center; gap: 8px; }
  .ac-section-head strong { font-size: 12px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .ac-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .ac-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
  }
  .ac-card.selectable { cursor: pointer; }
  .ac-card.selectable:hover { border-color: var(--line); }
  .ac-card.selected { border-color: var(--accent); }
  .ac-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ac-card-head strong {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px;
    color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ac-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .ac-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .ac-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .ac-badge.ok { border-color: var(--good); color: var(--good); }
  .ac-badge.warn { border-color: var(--warn); color: var(--warn); }
  .ac-badge.bad { border-color: var(--danger); color: var(--danger); }
  .ac-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .ac-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .ac-note { font-size: 12px; color: var(--muted); }
  .ac-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .ac-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-detail-head strong { font-size: 13px; color: var(--text); }
  .ac-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .ac-timeline { display: flex; flex-direction: column; gap: 0; }
  .ac-event {
    display: grid; grid-template-columns: 150px 120px minmax(0, 1fr); gap: 10px;
    padding: 6px 4px; border-bottom: 1px solid var(--line-soft); font-size: 12px;
  }
  .ac-event:last-child { border-bottom: 0; }
  .ac-event time { color: var(--muted); font-variant-numeric: tabular-nums; }
  .ac-event code { color: var(--accent); font-size: 11.5px; }
  .ac-event span { color: var(--text); overflow-wrap: anywhere; }
  .ac-confidence { font-size: 11.5px; color: var(--muted); }
  .ac-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(420px, calc(100vw - 32px));
  }
  .ac-dialog::backdrop { background: #05070bb3; }
  .ac-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .ac-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .ac-dialog select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .ac-dialog .ac-actions { justify-content: flex-end; margin-top: 12px; }

  html[data-viewport="tablet"] .ac-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="tablet"] .ac-event { grid-template-columns: 130px 110px minmax(0, 1fr); }

  html[data-viewport="mobile"] .ac-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
  html[data-viewport="mobile"] .ac-actions { width: 100%; }
  html[data-viewport="mobile"] .ac-actions button { flex: 1 1 auto; }
  html[data-viewport="mobile"] .ac-detail { padding: 10px; }

  @container (max-width: 640px) {
    .ac-grid { grid-template-columns: minmax(0, 1fr); }
    .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
    .ac-actions button { flex: 1 1 auto; }
  }
`;
