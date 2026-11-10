export const styles = `
  .home { display: flex; flex-direction: column; gap: 14px; max-width: 1100px; }
  .home-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .home h3 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -.01em; }
  .home-sub { margin: 2px 0 0; color: #8f98ab; font-size: 12.5px; }
  .home-actions button, .home-refresh {
    padding: 7px 11px; border-radius: 7px; border: 1px solid #2a3142; cursor: pointer;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px;
  }
  .home-actions button:hover, .home-refresh:hover { border-color: #3a4560; color: #fff; }
  .home-refresh:disabled { opacity: .5; cursor: default; }
  .home-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .home-card {
    border: 1px solid #202632; border-radius: 10px; background: #0f1219;
    padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; min-width: 0;
  }
  .home-card.wide { grid-column: 1 / -1; }
  .home-card h4 {
    margin: 0; color: #8f98ab; font-size: 11px; font-weight: 600;
    text-transform: uppercase; letter-spacing: .08em;
  }
  .home dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 6px 16px; margin: 0; font-size: 12.5px; }
  .home dt { color: #667087; }
  .home dd { margin: 0; color: #c8d0df; overflow-wrap: anywhere; }
  .home-bar { height: 6px; border-radius: 3px; background: #1b2130; overflow: hidden; display: flex; }
  .home-bar span { display: block; height: 100%; background: linear-gradient(90deg, #4361a8, #7aa2ff); }
  .home-bar.warm span { background: linear-gradient(90deg, #9a6a3a, #f7c948); }
  .home-bar-label { display: flex; justify-content: space-between; gap: 8px; font-size: 11px; color: #8f98ab; }
  .home-disk, .home-net {
    display: flex; flex-direction: column; gap: 5px; padding: 9px 0;
    border-top: 1px solid #171c26;
  }
  .home-disk:first-child, .home-net:first-child { border-top: 0; padding-top: 0; }
  .home-disk:last-child, .home-net:last-child { padding-bottom: 0; }
  .home-disk-head, .home-net-head { display: flex; justify-content: space-between; gap: 10px; align-items: baseline; }
  .home-disk-name, .home-net-name { color: #e8ebf2; font-weight: 600; font-size: 12.5px; }
  .home-disk-meta, .home-net-meta { color: #667087; font-size: 11px; }
  .home-net-state { text-transform: uppercase; font-size: 10px; letter-spacing: .06em; color: #9fe6cb; }
  .home-net-state.down { color: #ffb3ba; }
  .home-net-addr { font: 11px/1.5 ui-monospace, monospace; color: #b9c3d6; overflow-wrap: anywhere; }
  .home-net-io { color: #8f98ab; font-size: 11px; }
  .home-empty { color: #667087; font-size: 12px; }
  .home-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .home-status.bad { color: #ffb3ba; }
  @media (max-width: 720px) {
    .home-grid { grid-template-columns: 1fr; }
    .home-card.wide { grid-column: auto; }
  }
`;
