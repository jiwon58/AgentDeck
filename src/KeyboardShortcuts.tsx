const shortcuts = [
  ['Ctrl+T', 'New session'],
  ['Ctrl+W', 'Close active Deck tab'],
  ['Ctrl+Tab', 'Next Deck tab'],
  ['Ctrl+Shift+Tab', 'Previous Deck tab'],
  ['Ctrl+K', 'Find or switch sessions'],
  ['Ctrl+F', 'Find in output when the terminal toolbar has focus'],
] as const;

export function KeyboardShortcuts() {
  return <div className="stack">
    <section className="stack">
      <h3>Deck shortcuts</h3>
      <p className="muted">Focus a Deck control, such as the tab bar or a toolbar button, to use these shortcuts.</p>
      <dl className="shortcuts">{shortcuts.map(([key, action]) =>
        <div key={key}><dt><kbd>{key}</kbd></dt><dd>{action}</dd></div>)}</dl>
    </section>
    <section className="stack">
      <h3>Agent terminal</h3>
      <p>Click inside the terminal to use your Agent’s shortcuts. For example, <kbd>Ctrl+T</kbd> controls the Agent view and <kbd>Ctrl+W</kbd> edits input where supported.</p>
      <p className="muted">Right-click the terminal for Copy or Paste. Select the text first to copy it. Gajae-Code lists its active bindings with <code>/hotkeys</code>.</p>
      <dl className="shortcuts">
        <div><dt><kbd>Alt+↑</kbd> / <kbd>Alt+↓</kbd></dt><dd>GJC · recover or select a queued or steering message for editing</dd></div>
        <div><dt><kbd>Alt+Q</kbd></dt><dd>GJC · queue the draft on Windows</dd></div>
        <div><dt><kbd>Shift+Enter</kbd></dt><dd>GJC, Claude Code, Hermes · insert a newline</dd></div>
        <div><dt><kbd>Shift+Tab</kbd></dt><dd>GJC · thinking level. Claude Code · permission mode.</dd></div>
        <div><dt><kbd>Ctrl+T</kbd> / <kbd>F6</kbd></dt><dd>Hermes · live work monitor</dd></div>
      </dl>
    </section>
  </div>;
}

