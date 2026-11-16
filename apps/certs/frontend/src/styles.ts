export const styles = `
  .ct { display: flex; flex-direction: column; gap: 14px; }
  .ct-bar { display: flex; align-items: center; gap: 8px; }
  .ct-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; color: #8f98ab; font-size: 12px; }
  .grow { flex: 1; }
  .ct-bar button, .ct-actions button {
    padding: 6px 11px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .ct-bar button:hover, .ct-actions button:hover { border-color: #4a5c82; color: #fff; }
  .ct-summary { display: flex; flex-wrap: wrap; gap: 8px; }
  .ct-badge {
    display: inline-flex; align-items: center; gap: 5px; padding: 3px 8px;
    border-radius: 999px; border: 1px solid #2a3142; background: #11151d;
    color: #98a2b6; font-size: 11px;
  }
  .ct-badge.ok { border-color: #2f5a3f; color: #93d9ab; background: #122018; }
  .ct-badge.warn { border-color: #5a4a2a; color: #f3c969; background: #221c10; }
  .ct-badge.bad { border-color: #5c2b31; color: #ffb3ba; background: #241214; }
  .ct-badge.off { color: #7a8497; }
  .ct-table { display: flex; flex-direction: column; gap: 4px; }
  .ct-row {
    display: grid; grid-template-columns: minmax(160px, 2fr) 90px 80px 90px 90px 60px;
    gap: 10px; align-items: center; width: 100%; text-align: left;
    padding: 8px 10px; border-radius: 7px; border: 1px solid transparent;
    background: #11151d; color: #d6dcea; font: inherit; font-size: 12.5px; cursor: pointer;
  }
  .ct-row:hover { border-color: #2f3a52; }
  .ct-row.selected { border-color: #4a5c82; background: #151d2c; }
  .ct-row.head { background: transparent; color: #7a8497; font-size: 11px; cursor: default; }
  .ct-row.head:hover { border-color: transparent; }
  .ct-empty { color: #7a8497; font-size: 12.5px; }
  .ct-detail { display: flex; flex-direction: column; gap: 10px; padding: 12px; border-radius: 8px; border: 1px solid #2a3142; background: #0f1219; }
  .ct-detail h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .ct-detail dl { display: grid; grid-template-columns: 120px 1fr; gap: 6px 12px; margin: 0; font-size: 12px; }
  .ct-detail dt { color: #7a8497; }
  .ct-detail dd { margin: 0; color: #d6dcea; overflow-wrap: anywhere; }
  .ct-actions { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .ct-actions .danger { border-color: #5c2b31; color: #ffb3ba; }
  .ct-check { display: inline-flex; align-items: center; gap: 5px; color: #98a2b6; font-size: 12px; }
  .ct-status { min-height: 16px; color: #667087; font-size: 11.5px; margin: 0; }
  .ct-status.bad { color: #ffb3ba; }
  @media (max-width: 700px) {
    .ct-row { grid-template-columns: 1fr 80px 70px; }
    .ct-row > :nth-child(4), .ct-row > :nth-child(5), .ct-row > :nth-child(6) { display: none; }
  }
`;
