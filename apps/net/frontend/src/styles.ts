export const styles = `
  .net { display: flex; flex-direction: column; gap: 14px; min-height: 400px; }
  .net-bar { display: flex; align-items: center; gap: 10px; }
  .net-bar .grow { flex: 1; }
  .net-interval { display: flex; align-items: center; gap: 6px; color: #8f98ab; font-size: 12px; }
  .net-interval select {
    padding: 5px 8px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-bar button, .net-add button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .net-bar button:hover, .net-add button:hover { border-color: #3a4560; color: #fff; }
  .net-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 10px; }
  .net-card {
    border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 11px 12px;
    display: grid; gap: 7px;
  }
  .net-card-head { display: flex; align-items: center; gap: 8px; }
  .net-name { font-weight: 600; color: #e8ebf2; }
  .net-state { margin-left: auto; font-size: 10px; text-transform: uppercase; letter-spacing: .08em; }
  .net-state.up { color: #52d1a4; }
  .net-state.down { color: #667087; }
  .net-toggle { padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24; color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer; }
  .net-toggle:hover { border-color: #3a4560; color: #fff; }
  .net-meta { color: #667087; font: 11px ui-monospace, monospace; display: flex; gap: 12px; flex-wrap: wrap; }
  .net-addrs { display: flex; gap: 5px; flex-wrap: wrap; }
  .net-chip {
    padding: 2px 7px; border-radius: 999px; border: 1px solid #2a3142; color: #b9c3d6;
    font: 11px ui-monospace, monospace;
  }
  .net-chip.ipv6 { border-color: #9d7bff44; color: #cbb8ff; }
  .net-counters { display: flex; gap: 14px; color: #8f98ab; font-size: 11px; }
  .net-counters b { color: #c8d0df; font-weight: 500; }
  .net-routes { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .net-route {
    display: grid; grid-template-columns: 60px minmax(150px, 1fr) minmax(120px, 1fr) 100px 70px 30px;
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .net-route:last-child { border-bottom: 0; }
  .net-route.head {
    position: sticky; top: 0; background: #11151e; color: #667087; text-transform: uppercase;
    letter-spacing: .06em; font-size: 10px;
  }
  .net-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .net-remove {
    width: 22px; height: 22px; border: 0; border-radius: 5px; background: transparent;
    color: #667087; cursor: pointer; font-size: 13px; line-height: 1;
  }
  .net-remove:hover { background: #2b3344; color: #ff7d8a; }
  .net-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .net-add input, .net-add select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .net-add input[data-destination] { flex: 1; min-width: 120px; }
  .net-add input[data-gateway] { flex: 1; min-width: 110px; }
  .net-add input[data-metric] { width: 70px; }
  .net-add .hint { color: #667087; font-size: 11px; }
  .net-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .net-status.bad { color: #ffb3ba; }

  @media (max-width: 640px) {
    .net-bar { flex-wrap: wrap; }
    .net-bar .net-mono { flex: 1 0 100%; }
    .net-bar .grow { display: none; }
    .net-interval { margin-left: auto; }
    .net-routes { overflow: visible; border: 0; background: transparent; }
    .net-route.head { display: none; }
    .net-route {
      display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: center;
      position: relative; padding: 10px 40px 10px 12px; margin-bottom: 8px;
      border: 1px solid #202632; border-radius: 9px; background: #0d1016;
    }
    .net-route:last-child { margin-bottom: 0; }
    .net-route > span { display: inline-flex; gap: 4px; align-items: baseline; }
    .net-route > span[data-label]::before {
      content: attr(data-label); color: #667087; font-size: 9px;
      text-transform: uppercase; letter-spacing: .06em;
    }
    .net-route > :nth-child(2) { order: -1; flex: 1 0 100%; color: #e8ebf2; font-weight: 600; }
    .net-route > :nth-child(2)::before { content: none; }
    .net-route > :nth-child(6) { position: absolute; top: 6px; right: 6px; margin: 0; }
  }
`;
