export const styles = `
  .st { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .st-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .st-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .st-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .st button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .st button:hover { border-color: var(--accent); }
  .st button.primary { border-color: var(--accent); background: var(--panel-3); }
  .st button:disabled { opacity: .55; cursor: default; }
  .st input[type="search"] {
    padding: 5px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px; min-width: 180px; flex: 1 1 220px;
  }
  .st-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .st-status.bad { color: var(--danger); }
  .st-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .st-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0; cursor: pointer;
  }
  .st-card:hover { border-color: var(--line); }
  .st-card.selected { border-color: var(--accent); }
  .st-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .st-initials {
    width: 30px; height: 30px; border-radius: 8px; display: grid; place-items: center;
    background: var(--panel-3); color: var(--accent); font-size: 12px; font-weight: 600;
  }
  .st-card-head strong { font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .st-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .st-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .st-badge.ok { border-color: var(--good); color: var(--good); }
  .st-badge.warn { border-color: var(--warn); color: var(--warn); }
  .st-badge.bad { border-color: var(--danger); color: var(--danger); }
  .st-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px; color: var(--muted); font-size: 12px;
  }
  .st-note { font-size: 12px; color: var(--muted); }
  .st-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .st-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .st-detail-head strong { font-size: 13px; color: var(--text); }
  .st-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .st-versions { display: flex; flex-direction: column; gap: 4px; }
  .st-version {
    display: grid; grid-template-columns: 110px 80px 90px 1fr 90px;
    gap: 8px; align-items: center; padding: 5px 4px; border-bottom: 1px solid var(--line-soft); font-size: 12px;
  }
  .st-version:last-child { border-bottom: 0; }
  .st-version .yanked { color: var(--danger); }
  .st-sources { display: flex; flex-direction: column; gap: 8px; }
  .st-source {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    border: 1px solid var(--line-soft); border-radius: 9px; padding: 8px 10px; background: var(--bg-soft);
  }

  html[data-viewport="mobile"] .st-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .st-version { grid-template-columns: minmax(0, 1fr) 80px; }
  html[data-viewport="mobile"] .st-version > :nth-child(3),
  html[data-viewport="mobile"] .st-version > :nth-child(4) { display: none; }

  @container (max-width: 640px) {
    .st-grid { grid-template-columns: minmax(0, 1fr); }
  }
`;
