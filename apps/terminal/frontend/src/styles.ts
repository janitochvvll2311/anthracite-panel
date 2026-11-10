export const styles = `
  .term { position: relative; display: flex; flex-direction: column; gap: 10px; height: 100%; min-height: 0; }
  .term-bar { display: flex; align-items: center; gap: 8px; flex: none; font-size: 12px; color: #667087; }
  .term-bar .who { color: #8f98ab; }
  .term-bar .state { margin-left: auto; }
  .term-controls { display: flex; align-items: center; gap: 4px; }
  .term-bar button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .term-bar button:hover { border-color: #3a4560; color: #fff; }
  .term-bar button:disabled { opacity: .4; cursor: default; }
  .term-screen {
    position: relative; flex: 1; min-height: 0; margin: 0; padding: 0; overflow: hidden;
    border: 1px solid #202632; border-radius: 9px; background: #080a0f;
    touch-action: pan-y; cursor: text;
  }
  .term-screen .xterm { height: 100%; padding: 10px 12px; }
  .term-screen .xterm-viewport { background: transparent !important; }
  .term-keys { display: none; }
  .term-keys button {
    flex: none; min-width: 34px; height: 36px; padding: 0 7px;
    border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: 11px ui-monospace, monospace; white-space: nowrap;
    cursor: pointer;
  }
  .term-keys button:hover { border-color: #3a4560; color: #fff; }
  .term-keys button.armed { background: #24406e; border-color: #7aa2ff; color: #fff; }
  .term-keys button:disabled { opacity: .4; cursor: default; }
  .term-keys button svg { display: block; width: 15px; height: 15px; }
  .term-input {
    position: absolute; left: 8px; bottom: 8px; width: 1px; height: 1px;
    padding: 0; border: 0; opacity: 0; pointer-events: none;
    font-size: 16px; caret-color: transparent; outline: none;
    overflow: hidden; white-space: pre;
  }
  .term-status { margin: 0; min-height: 16px; flex: none; font-size: 12px; color: #667087; }
  .term-status.bad { color: #ffb3ba; }
  @media (max-width: 560px) {
    .term { gap: 8px; }
    .term-bar {
      display: grid; grid-template-columns: minmax(0, 1fr) auto;
      align-items: center; gap: 6px 8px;
    }
    .term-bar .who {
      grid-column: 1; grid-row: 1; min-width: 0;
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .term-bar .state {
      grid-column: 2; grid-row: 1; margin-left: 0;
      font-size: 11px; white-space: nowrap;
    }
    .term-controls {
      grid-column: 1 / -1; grid-row: 2;
      display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px;
    }
    .term-controls button {
      width: 100%; min-height: 38px; padding: 0 6px;
      font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .term-keys { display: flex; gap: 6px; flex: none; min-width: 0; }
    .term-modifiers {
      display: flex; gap: 4px; flex: none; padding-right: 6px;
      border-right: 1px solid #202634;
    }
    .term-key-scroll {
      display: flex; gap: 4px; flex: 1; min-width: 0; overflow-x: auto;
      overflow-y: hidden; overscroll-behavior-x: contain; touch-action: pan-x;
      -webkit-overflow-scrolling: touch; scrollbar-width: thin;
    }
    .term-keys button.armed { box-shadow: inset 0 0 0 1px #7aa2ff; }
    .term-screen .xterm { padding: 8px 10px; }
    .term-status { font-size: 11.5px; }
  }
`;
