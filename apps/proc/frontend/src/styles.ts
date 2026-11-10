export const styles = `
  .proc { display: flex; flex-direction: column; gap: 10px; min-height: 380px; }
  .proc-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .proc-count { color: #667087; font-size: 12px; }
  .proc-bar .grow { flex: 1; }
  .proc-bar button, .proc-bar select, .proc-spawn button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-bar button:hover, .proc-spawn button:hover { border-color: #3a4560; color: #fff; }
  .proc-check { display: flex; align-items: center; gap: 5px; color: #8f98ab; font-size: 12px; }
  .proc-spawn { display: flex; gap: 6px; }
  .proc-spawn input {
    flex: 1; min-width: 160px; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .proc-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .proc-row {
    display: grid; grid-template-columns: 64px 64px minmax(160px, 1.4fr) 90px 90px 90px 90px 66px;
    gap: 8px; align-items: center; padding: 7px 10px; border-bottom: 1px solid #171c26;
    font-size: 12px; min-width: 820px;
  }
  .proc-row:last-child { border-bottom: 0; }
  .proc-row.head {
    position: sticky; top: 0; z-index: 1; background: #11151e; color: #667087;
    text-transform: uppercase; letter-spacing: .06em; font-size: 10px;
  }
  .proc-row:hover:not(.head) { background: #151a24; }
  .proc-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #e8ebf2; }
  .proc-user, .proc-mono { color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px; }
  .proc-state { text-transform: capitalize; }
  .proc-state.running { color: #52d1a4; }
  .proc-state.sleeping { color: #8f98ab; }
  .proc-state.stopped { color: #f3c969; }
  .proc-state.zombie { color: #ff7d8a; }
  .proc-bar-track { height: 5px; border-radius: 999px; background: #202634; overflow: hidden; flex: 1; }
  .proc-bar-fill { height: 100%; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .proc-cpu { display: flex; align-items: center; gap: 6px; }
  .proc-cpu .percent { width: 38px; text-align: right; color: #8f98ab; font: 11px ui-monospace, monospace; }
  .proc-signal {
    padding: 4px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .proc-signal:hover { border-color: #ff7d8a88; color: #ffb3ba; }
  .proc-more { display: flex; justify-content: center; }
  .proc-more button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .proc-status.bad { color: #ffb3ba; }
`;
