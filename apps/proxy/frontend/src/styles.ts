export const styles = `
  .px { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .px-field { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .px-field-name {
    display: none; font-size: 10px; letter-spacing: .04em; text-transform: uppercase;
    color: var(--muted-2);
  }
  .px-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .px-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .px-tabs { display: flex; gap: 4px; }
  .px-tabs button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--muted); font: inherit; font-size: 12px; cursor: pointer;
  }
  .px-tabs button.active { border-color: var(--accent); background: var(--panel-2); color: var(--text); }
  .grow { flex: 1; }
  .px button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .px button:hover { border-color: var(--accent); color: var(--text); }
  .px button.primary { border-color: var(--accent); background: var(--panel-3); color: var(--text); }
  .px button.danger { border-color: var(--danger); color: var(--danger); }
  .px-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .px-status.bad { color: var(--danger); }
  .px-banner {
    padding: 8px 10px; border-radius: 7px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .px-note { font-size: 12px; color: var(--muted); }
  .px-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .px-section { display: flex; flex-direction: column; gap: 12px; }
  .px-card {
    border: 1px solid var(--line-soft); border-radius: 9px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 8px;
  }
  .px-card-head { display: flex; align-items: center; gap: 8px; }
  .px-card-head strong { color: var(--text); font-size: 13px; }
  .px-grid {
    display: grid; grid-template-columns: 90px 1fr 90px 90px 70px 140px;
    gap: 8px; align-items: center; font-size: 12px;
  }
  .px-grid.hosts { grid-template-columns: 90px 1fr 140px 70px 70px 70px 60px; }
  .px-grid.head { color: var(--muted-2); font-size: 11px; }
  .px-grid input[type="text"], .px-grid input[type="number"], .px-grid select {
    width: 100%; padding: 4px 6px; border-radius: 6px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px;
  }
  .px-form {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap;
    padding-top: 4px; border-top: 1px dashed var(--line-soft);
  }
  .px-form input[type="text"], .px-form input[type="number"], .px-form select {
    padding: 4px 6px; border-radius: 6px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px;
  }
  .px-check { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); }
  .px-log {
    display: flex; flex-direction: column; gap: 2px; max-height: 46vh;
    overflow: auto; border: 1px solid var(--line-soft); border-radius: 8px; background: var(--bg);
  }
  .px-event {
    display: grid; grid-template-columns: 64px 120px 60px 1fr 60px 60px 70px;
    gap: 8px; padding: 4px 8px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; color: var(--text); border-bottom: 1px solid var(--line-soft);
  }
  .px-event .bad { color: var(--danger); }
  .px-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted);
  }
  .px-badge.ok { border-color: var(--good); color: var(--good); }
  .px-badge.bad { border-color: var(--danger); color: var(--danger); }
  .px-empty { padding: 10px; color: var(--muted-2); font-size: 12px; }
  .px-card { min-width: 0; }

  /* The panel's explicit developer modes reach the frame as
     html[data-viewport], so the view adapts even when the iframe stays wide
     on a desktop browser. */
  html[data-viewport="tablet"] .px-grid,
  html[data-viewport="tablet"] .px-grid.hosts { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  html[data-viewport="tablet"] .px-grid.head { display: none; }
  html[data-viewport="tablet"] .px-field-name { display: block; }
  html[data-viewport="tablet"] .px-card-head { flex-wrap: wrap; }
  html[data-viewport="tablet"] .px-event { grid-template-columns: 60px 1fr 60px 60px; }
  html[data-viewport="tablet"] .px-event > :nth-child(3),
  html[data-viewport="tablet"] .px-event > :nth-child(6),
  html[data-viewport="tablet"] .px-event > :nth-child(7) { display: none; }

  html[data-viewport="mobile"] .px-grid,
  html[data-viewport="mobile"] .px-grid.hosts { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .px-grid.head { display: none; }
  html[data-viewport="mobile"] .px-field-name { display: block; }
  html[data-viewport="mobile"] .px-card-head { flex-wrap: wrap; }
  html[data-viewport="mobile"] .px-event { grid-template-columns: 58px 1fr 52px; }
  html[data-viewport="mobile"] .px-event > :nth-child(2) { display: none; }
  html[data-viewport="mobile"] .px-form input[type="text"],
  html[data-viewport="mobile"] .px-form input[type="number"],
  html[data-viewport="mobile"] .px-form select { flex: 1 1 100%; }
  html[data-viewport="mobile"] .px-form button { width: 100%; }
  html[data-viewport="mobile"] .px-actions { width: 100%; }

  /* The view is responsive to the frame width, which is what the panel's
     desktop/tablet/mobile modes change. Containers, not the page width: the
     app lives in an iframe. */
  @container (max-width: 980px) {
    .px-grid, .px-grid.hosts { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .px-grid.head { display: none; }
    .px-field-name { display: block; }
    .px-card-head { flex-wrap: wrap; }
    .px-event { grid-template-columns: 60px 1fr 60px 60px; }
    .px-event > :nth-child(3),
    .px-event > :nth-child(6),
    .px-event > :nth-child(7) { display: none; }
  }

  @container (max-width: 640px) {
    .px-grid, .px-grid.hosts { grid-template-columns: minmax(0, 1fr); }
    .px-event { grid-template-columns: 58px 1fr 52px; }
    .px-event > :nth-child(2) { display: none; }
    .px-form input[type="text"],
    .px-form input[type="number"],
    .px-form select { flex: 1 1 100%; }
    .px-form button { width: 100%; }
    .px-actions { width: 100%; }
  }
`;
