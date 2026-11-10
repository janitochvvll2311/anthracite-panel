export const styles = `
  .term { display: flex; flex-direction: column; gap: 10px; min-height: 380px; }
  .term-bar { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #667087; }
  .term-bar .who { color: #8f98ab; }
  .term-bar .state { margin-left: auto; }
  .term-bar button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .term-bar button:hover { border-color: #3a4560; color: #fff; }
  .term-bar button:disabled { opacity: .4; cursor: default; }
  .term-screen {
    flex: 1; min-height: 320px; margin: 0; padding: 12px 14px; overflow: auto;
    border: 1px solid #202632; border-radius: 9px; background: #080a0f;
    font: 12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; color: #c8d0df;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .term-form { display: flex; gap: 8px; }
  .term-form input {
    flex: 1; padding: 8px 11px; border-radius: 8px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12.5px ui-monospace, monospace;
  }
  .term-form input:focus { outline: 2px solid #7aa2ff66; outline-offset: 1px; border-color: #3a4560; }
  .term-form input:disabled { opacity: .55; cursor: not-allowed; }
  .term-form button:disabled { opacity: .45; cursor: not-allowed; }
  .term-form button {
    padding: 8px 13px; border-radius: 8px; border: 1px solid #3a4560; cursor: pointer;
    background: linear-gradient(180deg, #2a3550, #222b3f); color: #e8ebf2; font: inherit;
  }
  .term-status { margin: 0; min-height: 16px; font-size: 12px; color: #667087; }
  .term-status.bad { color: #ffb3ba; }
`;
