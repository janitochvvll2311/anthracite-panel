export const styles = `
  .fw { display: flex; flex-direction: column; gap: 14px; min-height: 360px; }
  .fw-bar { display: flex; align-items: center; gap: 10px; }
  .fw-bar .grow { flex: 1; }
  .fw-bar button, .fw button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fw-bar button:hover, .fw button:hover { border-color: #3a4560; color: #fff; }
  .fw-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .fw-policy, .fw-chains { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
  .fw-card { border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 10px 12px; min-width: 180px; }
  .fw-card-head { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .fw-card-head strong { color: #e8ebf2; font-size: 13px; }
  .fw-badge { padding: 1px 6px; border-radius: 999px; border: 1px solid #2a3142; font-size: 9.5px; text-transform: uppercase; letter-spacing: .07em; color: #8f98ab; }
  .fw-badge.allow { border-color: #6fcf9744; color: #9fe0b8; }
  .fw-badge.deny { border-color: #ff7d8a44; color: #ffb3ba; }
  .fw-badge.custom { border-color: #d8b45a44; color: #e5ca83; }
  .fw-badge.on { border-color: #6fcf9744; color: #9fe0b8; }
  .fw-badge.off { border-color: #ff7d8a44; color: #ffb3ba; }
  .fw-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .fw-row { display: grid; grid-template-columns: 50px 70px 70px 90px minmax(120px, 1fr) minmax(120px, auto); gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center; }
  .fw-row:last-child { border-bottom: 0; }
  .fw-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .fw-actions { display: flex; gap: 4px; flex-wrap: wrap; }
  .fw-btn { padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer; }
  .fw-btn.danger:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .fw-form { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fw-form select, .fw-form input {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .fw-form input[data-port] { width: 76px; }
  .fw-form input[data-comment] { flex: 1; min-width: 130px; }
  .fw-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .fw-status.bad { color: #ffb3ba; }
  .fw-warning { color: #e5ca83; font-size: 12px; }

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
