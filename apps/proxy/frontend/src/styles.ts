export const styles = `
  .px { display: flex; flex-direction: column; gap: 14px; }
  .px-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .px-tabs { display: flex; gap: 4px; }
  .px-tabs button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #11151d; color: #98a2b6; font: inherit; font-size: 12px; cursor: pointer;
  }
  .px-tabs button.active { border-color: #4a5c82; background: #151d2c; color: #fff; }
  .grow { flex: 1; }
  .px button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .px button:hover { border-color: #4a5c82; color: #fff; }
  .px button.primary { border-color: #33507a; background: #16233a; color: #dbe6ff; }
  .px button.danger { border-color: #5c2b31; color: #ffb3ba; }
  .px-status { margin: 0; font-size: 12px; color: #8f98ab; min-height: 16px; }
  .px-status.bad { color: #ffb3ba; }
  .px-banner {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #5c2b31;
    background: #241214; color: #ffb3ba; font-size: 12px;
  }
  .px-note { font-size: 12px; color: #8f98ab; }
  .px-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: #8f98ab; }
  .px-section { display: flex; flex-direction: column; gap: 12px; }
  .px-card {
    border: 1px solid #232a38; border-radius: 9px; padding: 10px 12px;
    background: #0f131a; display: flex; flex-direction: column; gap: 8px;
  }
  .px-card-head { display: flex; align-items: center; gap: 8px; }
  .px-card-head strong { color: #dfe5f1; font-size: 13px; }
  .px-grid {
    display: grid; grid-template-columns: 90px 1fr 90px 90px 70px 140px;
    gap: 8px; align-items: center; font-size: 12px;
  }
  .px-grid.hosts { grid-template-columns: 90px 1fr 140px 70px 70px 70px 60px; }
  .px-grid.head { color: #7a8497; font-size: 11px; }
  .px-grid input[type="text"], .px-grid input[type="number"], .px-grid select {
    width: 100%; padding: 4px 6px; border-radius: 6px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12px;
  }
  .px-form {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap;
    padding-top: 4px; border-top: 1px dashed #232a38;
  }
  .px-form input[type="text"], .px-form input[type="number"], .px-form select {
    padding: 4px 6px; border-radius: 6px; border: 1px solid #2a3142;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12px;
  }
  .px-check { display: inline-flex; align-items: center; gap: 4px; color: #98a2b6; }
  .px-log {
    display: flex; flex-direction: column; gap: 2px; max-height: 46vh;
    overflow: auto; border: 1px solid #232a38; border-radius: 8px; background: #0d1117;
  }
  .px-event {
    display: grid; grid-template-columns: 64px 120px 60px 1fr 60px 60px 70px;
    gap: 8px; padding: 4px 8px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; color: #b9c2d4; border-bottom: 1px solid #141a24;
  }
  .px-event .bad { color: #ffb3ba; }
  .px-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid #2a3142;
    font-size: 11px; color: #98a2b6;
  }
  .px-badge.ok { border-color: #2f5a3f; color: #93d9ab; }
  .px-badge.bad { border-color: #5c2b31; color: #ffb3ba; }
  .px-empty { padding: 10px; color: #7a8497; font-size: 12px; }
`;
