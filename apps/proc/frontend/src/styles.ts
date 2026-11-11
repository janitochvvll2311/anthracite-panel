export const styles = `
  .proc { display: flex; flex-direction: column; gap: 10px; min-height: 380px; }
  .proc.fill { height: 100%; min-height: 0; padding: 14px 16px; }
  .proc-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .proc-count { color: #667087; font-size: 12px; }
  .proc-bar .grow { flex: 1; }
  .proc-bar button, .proc-bar select, .proc-spawn button, .proc-mobile-tools button, .proc-mobile-tools select {
    padding: 6px 10px; min-height: 34px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-bar button:hover, .proc-spawn button:hover { border-color: #3a4560; color: #fff; }
  .proc-tabs { display: inline-flex; border: 1px solid #2a3142; border-radius: 8px; overflow: hidden; }
  .proc-tabs button { border: 0; border-radius: 0; background: transparent; }
  .proc-tabs button[aria-pressed="true"] { background: #1f2940; color: #fff; }
  .proc-fold { font-size: 11px !important; }
  .proc-interval { display: flex; align-items: center; gap: 6px; color: #8f98ab; font-size: 12px; }
  .proc-interval select { padding: 5px 8px; min-height: 34px; }
  .proc-spawn { display: flex; gap: 6px; }
  .proc-spawn input {
    flex: 1; min-width: 0; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .proc-mobile-tools { display: none; }

  .proc-table {
    border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016;
    max-height: min(70vh, 760px);
  }
  .proc.fill .proc-table { flex: 1 1 auto; min-height: 0; max-height: none; }
  .proc-head { position: sticky; top: 0; z-index: 2; background: #0d1016; }
  .proc-body { min-width: 0; }
  .proc-body .proc-row { height: 40px; box-sizing: border-box; }
  .proc-spacer { pointer-events: none; }
  .proc-row {
    display: grid; gap: 8px; align-items: center; padding: 7px 10px;
    border-bottom: 1px solid #171c26; font-size: 12px;
  }
  .proc-row.flat { grid-template-columns: 64px 64px minmax(160px, 1.4fr) 90px 90px 100px 80px 66px; }
  .proc-row.tree { grid-template-columns: minmax(220px, 1fr) 90px 90px 100px 80px 66px; }
  .proc-row:last-child { border-bottom: 0; }
  .proc-row.head {
    background: #11151e; color: #667087;
    text-transform: uppercase; letter-spacing: .06em; font-size: 10px;
  }
  .proc-row.head .sortable { cursor: pointer; user-select: none; }
  .proc-row.head .sortable:focus-visible { outline: 2px solid #7aa2ff; outline-offset: 2px; border-radius: 3px; }
  .proc-row.head .sortable:hover { color: #c8d0df; }
  .proc-row.head .sorted { color: #e8ebf2; }
  .proc-row.filters { padding: 4px 10px; background: #10141c; border-bottom: 1px solid #202632; }
  .proc-filter-cell { min-width: 0; }
  .proc-filter-cell input {
    width: 100%; min-width: 0; padding: 3px 6px; border-radius: 5px; border: 1px solid #2a3142;
    background: #0b0e14; color: #e8ebf2; font: 11px ui-monospace, monospace;
  }
  .proc-filter-cell input::placeholder { color: #4d5668; }
  .proc-row:hover:not(.head):not(.filters) { background: #151a24; }
  .proc-row.flat .identity { display: contents; }
  .proc-row.tree .identity { display: flex; align-items: center; min-width: 0; }
  .proc-row .proc-meta { display: contents; }
  .proc-cell { min-width: 0; }
  .proc-pid, .proc-ppid {
    color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px;
  }
  .proc-row.tree .proc-pid { margin-left: 3px; margin-right: 8px; min-width: 3ch; }
  .proc-name {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #e8ebf2;
    min-width: 0; flex: 1;
  }
  .proc-gutter { display: flex; flex: none; align-self: stretch; }
  .proc-guide {
    position: relative; width: 16px; flex: none; align-self: stretch;
    margin-top: -7px; margin-bottom: -8px;
  }
  .proc-guide::before {
    content: ""; position: absolute; left: 7px; top: 0; bottom: 0;
    border-left: 1px solid #2a3142;
  }
  .proc-guide.blank::before { display: none; }
  .proc-connector {
    position: relative; width: 16px; flex: none; align-self: stretch;
    margin-top: -7px; margin-bottom: -8px;
  }
  .proc-connector::before {
    content: ""; position: absolute; left: 7px; top: 0; bottom: 0;
    border-left: 1px solid #2a3142;
  }
  .proc-connector.last::before { bottom: 50%; }
  .proc-connector::after {
    content: ""; position: absolute; left: 7px; top: 50%; width: 8px;
    border-top: 1px solid #2a3142;
  }
  .proc-twisty {
    width: 20px; height: 20px; padding: 0; flex: none; border: 0; background: transparent;
    color: #8f98ab; font: 11px/1 ui-monospace, monospace; cursor: pointer;
  }
  .proc-twisty:hover { color: #e8ebf2; }
  .proc-twisty.empty { visibility: hidden; }
  .proc-cell.user, .proc-cell.memory {
    color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px;
  }
  .proc-cell.state { text-transform: capitalize; }
  .proc-cell.state.running { color: #52d1a4; }
  .proc-cell.state.sleeping { color: #8f98ab; }
  .proc-cell.state.stopped { color: #f3c969; }
  .proc-cell.state.zombie { color: #ff7d8a; }
  .proc-bar-track { height: 5px; border-radius: 999px; background: #202634; overflow: hidden; flex: 1; }
  .proc-bar-fill { height: 100%; display: block; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .proc-cell.cpu { display: flex; align-items: center; gap: 6px; }
  .proc-cell.cpu .percent { width: 38px; text-align: right; color: #8f98ab; font: 11px ui-monospace, monospace; }
  .proc-cell[data-label]::before { content: none; }
  .proc-signal {
    padding: 4px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .proc-signal:hover { border-color: #ff7d8a88; color: #ffb3ba; }
  .proc-more { display: flex; justify-content: center; }
  .proc-more button {
    padding: 6px 12px; min-height: 34px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .proc-status.bad { color: #ffb3ba; }
  .proc-empty { padding: 18px; color: #667087; text-align: center; font-size: 12px; }

  .proc.compact .proc-bar { gap: 8px; }
  .proc.compact .proc-count { flex: 1 0 100%; }
  .proc.compact .proc-bar .grow { display: none; }
  .proc.compact .proc-tabs { display: none; }
  .proc.compact .proc-interval { order: 1; }
  .proc.compact .proc-pick { order: 2; }
  .proc.compact [data-refresh] { order: 3; }
  .proc.compact .proc-fold { display: none; }
  .proc.compact .proc-mobile-tools {
    display: flex; gap: 6px; flex-wrap: wrap; align-items: center;
  }
  .proc.compact .proc-mobile-tools input {
    flex: 1 1 140px; min-width: 0; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .proc.compact .proc-mobile-tools select { flex: 0 0 auto; }
  .proc.compact .proc-direction { min-width: 54px; }
  .proc.compact .proc-spawn { width: 100%; }
  .proc.compact .proc-table { max-height: none; border: 0; background: transparent; overflow: visible; }
  .proc.compact.fill { padding: 10px; }
  .proc.compact.fill .proc-table { flex: 1 1 auto; min-height: 0; overflow: auto; }
  .proc.compact .proc-head { display: none; }
  .proc.compact .proc-body .proc-row { height: auto; }
  .proc.compact .proc-row.flat,
  .proc.compact .proc-row.tree {
    display: grid; grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas: "identity action" "meta action";
    gap: 6px 10px; margin-bottom: 8px; padding: 10px 12px;
    border: 1px solid #202632; border-radius: 10px; background: #0d1016;
  }
  .proc.compact .proc-row .identity {
    display: flex; grid-area: identity; min-width: 0; align-items: center;
    justify-content: flex-start; gap: 6px;
  }
  .proc.compact .proc-row .proc-pid::before { content: "PID "; color: #667087; }
  .proc.compact .proc-row .proc-ppid::before { content: "PPID "; color: #667087; }
  .proc.compact .proc-row .proc-name { margin-left: 2px; }
  .proc.compact .proc-row .proc-meta {
    display: flex; grid-area: meta; flex-wrap: wrap; align-items: center; gap: 2px 10px;
  }
  .proc.compact .proc-row .action { grid-area: action; align-self: start; }
  .proc.compact .proc-cell[data-label]::before {
    content: attr(data-label) " "; color: #667087; font-family: inherit;
    text-transform: none; letter-spacing: 0;
  }
  .proc.compact .proc-meta .ppid,
  .proc.compact .proc-meta .user,
  .proc.compact .proc-meta .state {
    border-left: 1px solid #202632; padding-left: 8px;
  }
  .proc.compact .proc-cell.cpu { flex: 1 0 100%; max-width: 240px; }
  .proc.compact .proc-name {
    white-space: normal; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical;
  }
  .proc.compact .proc-cell.action .proc-signal { min-height: 40px; padding: 8px 12px; font-size: 12px; }

  .confirm-dialog { border: 0; padding: 0; background: transparent; color: #e8ebf2; }
  .confirm-dialog::backdrop { background: rgba(5, 7, 11, .72); backdrop-filter: blur(2px); }
  .confirm-card {
    width: min(360px, calc(100vw - 32px)); display: flex; flex-direction: column; gap: 10px;
    padding: 16px; border: 1px solid #2a3142; border-radius: 12px; background: #11151e;
    box-shadow: 0 20px 60px #0008;
  }
  .confirm-card h3 { margin: 0; font-size: 15px; font-weight: 600; }
  .confirm-card p { margin: 0; color: #9aa4b8; font-size: 12.5px; line-height: 1.5; }
  .confirm-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .confirm-actions button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .confirm-actions .confirm-ok { border-color: #3a6ea5; background: #1d2b40; color: #dce9ff; }
  .confirm-actions .confirm-ok.danger { border-color: #7a3a44; background: #3a1c22; color: #ffb3ba; }
`;
