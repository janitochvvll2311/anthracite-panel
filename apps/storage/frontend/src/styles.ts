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

  @media (max-width: 640px) {
    .st-table { overflow: visible; border: 0; background: transparent; }
    .st-row.head { display: none; }
    .st-row {
      display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: center;
      padding: 10px 12px; margin-bottom: 8px;
      border: 1px solid #202632; border-radius: 9px; background: #0d1016;
    }
    .st-row:last-child { margin-bottom: 0; }
    .st-row > span { display: inline-flex; gap: 4px; align-items: baseline; }
    .st-row > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .st-row > :nth-child(1) { flex: 1 0 100%; color: #e8ebf2; font-weight: 600; }
    .st-row > :nth-child(1)::before { content: none; }
  }

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
