export const styles = `
  .acc { display: flex; flex-direction: column; gap: 15px; min-height: 360px; }
  .acc-bar { display: flex; align-items: center; gap: 10px; }
  .acc-bar .grow { flex: 1; }
  .acc-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .acc-bar button:hover { border-color: #3a4560; color: #fff; }
  .acc-section { display: grid; gap: 7px; }
  .acc-title { color: #667087; font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
  .acc-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .acc-row {
    display: grid; grid-template-columns: minmax(150px, 1.3fr) 55px 55px minmax(80px, .8fr) minmax(80px, .8fr) minmax(120px, 1fr) 90px minmax(190px, auto);
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .acc-row:last-child { border-bottom: 0; }
  .acc-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .acc-row.group { grid-template-columns: minmax(150px, 1fr) 70px minmax(200px, 3fr) minmax(80px, auto); }
  .acc-name { color: #e8ebf2; display: flex; align-items: center; gap: 7px; }
  .acc-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .acc-badge {
    padding: 1px 6px; border-radius: 999px; border: 1px solid #2a3142; font-size: 9.5px;
    text-transform: uppercase; letter-spacing: .07em; color: #8f98ab;
  }
  .acc-badge.system { border-color: #7aa2ff44; color: #9ab8ff; }
  .acc-badge.locked { border-color: #ff7d8a44; color: #ffb3ba; }
  .acc-badge.nss { border-color: #d8b45a44; color: #e5ca83; }
  .acc-badge.unknown { border-color: #8f98ab44; color: #b7bfce; }
  .acc-badge.policy.on { border-color: #6fcf9744; color: #9fe0b8; }
  .acc-badge.policy.off { border-color: #ff7d8a44; color: #ffb3ba; }
  .acc-policy { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .acc-members { color: #8f98ab; font-size: 11.5px; }
  .acc-actions { display: flex; gap: 4px; flex-wrap: wrap; }
  .acc-btn {
    padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer;
  }
  .acc-btn:hover { border-color: #3a4560; color: #fff; }
  .acc-btn.danger:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .acc-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .acc-add input, .acc-add button {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .acc-add input[data-name] { width: 120px; }
  .acc-add input[data-uid], .acc-add input[data-gid] { width: 76px; }
  .acc-add input[data-home] { flex: 1; min-width: 140px; }
  .acc-add input[data-gecos] { width: 140px; }
  .acc-add input[data-group-name] { width: 140px; }
  .acc-add input[data-shell] { width: 110px; }
  .acc-add button { background: #151a24; cursor: pointer; }
  .acc-add button:hover { border-color: #3a4560; color: #fff; }
  .acc-add .check { display: flex; align-items: center; gap: 5px; color: #8f98ab; font-size: 11px; }
  .acc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .acc-status.bad { color: #ffb3ba; }
  @media (max-width: 900px) {
    .acc-bar { flex-wrap: wrap; }
    .acc-table { border: 0; background: transparent; overflow: visible; }
    .acc-row.head { display: none; }
    .acc-row:not(.head) {
      display: grid; grid-template-columns: minmax(0, 1fr); gap: 4px;
      padding: 10px 12px; margin-bottom: 8px; border: 1px solid #202632;
      border-radius: 9px; background: #0f1219;
    }
    .acc-row.group { grid-template-columns: minmax(0, 1fr); }
    .acc-row:not(.head) > span:not(.acc-name):not(.acc-actions) {
      display: flex; justify-content: space-between; gap: 12px; font-size: 11.5px;
    }
    .acc-row:not(.head) > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9.5px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .acc-name { font-size: 13.5px; }
    .acc-actions { margin-top: 6px; padding-top: 6px; border-top: 1px solid #171c26; }
    .acc-add { align-items: stretch; }
    .acc-add input { width: 100% !important; }
    .acc-add button { width: 100%; }
  }
`;
