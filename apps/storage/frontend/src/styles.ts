export const styles = `
  .st { display: flex; flex-direction: column; gap: 14px; min-height: 360px; }
  .st-bar { display: flex; align-items: center; gap: 10px; }
  .st-bar .grow { flex: 1; }
  .st-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .st-bar button:hover { border-color: #3a4560; color: #fff; }
  .st-mounts { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 10px; }
  .st-card { border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 11px 12px; display: grid; gap: 8px; }
  .st-mount-head { display: flex; align-items: baseline; gap: 8px; }
  .st-unmount {
    margin-left: auto; padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142;
    background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer;
  }
  .st-unmount:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .st-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .st-add input, .st-add select, .st-add button {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .st-add button { background: #151a24; cursor: pointer; }
  .st-add button:hover { border-color: #3a4560; color: #fff; }
  .st-add input[data-mountpoint] { flex: 1; min-width: 160px; }
  .st-add input[data-filesystem] { width: 100px; }
  .st-mountpoint { font-weight: 600; color: #e8ebf2; }
  .st-fs { color: #cbb8ff; font: 11px ui-monospace, monospace; }
  .st-device { color: #667087; font: 11px ui-monospace, monospace; }
  .st-track { height: 7px; border-radius: 999px; background: #202634; overflow: hidden; }
  .st-fill { height: 100%; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .st-fill.warn { background: linear-gradient(90deg, #f3c969, #ff9f5a); }
  .st-fill.full { background: linear-gradient(90deg, #ff7d8a, #c2415a); }
  .st-numbers { display: flex; justify-content: space-between; color: #8f98ab; font-size: 11px; }
  .st-numbers b { color: #c8d0df; font-weight: 500; }
  .st-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .st-row {
    display: grid; grid-template-columns: minmax(120px, 1fr) 90px minmax(110px, 1fr) 90px 80px;
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .st-row:last-child { border-bottom: 0; }
  .st-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .st-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .st-badge { padding: 2px 7px; border-radius: 999px; border: 1px solid #2a3142; font-size: 10px; color: #8f98ab; }
  .st-badge.yes { border-color: #52d1a444; color: #9fe6cb; }
  .st-badge.no { color: #667087; }
  .st-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .st-status.bad { color: #ffb3ba; }
`;
