const shortcuts = [
  ['Ctrl+T', 'New session'],
  ['Ctrl+W', 'Close active Deck tab'],
  ['Ctrl+Tab', 'Next Deck tab'],
  ['Ctrl+Shift+Tab', 'Previous Deck tab'],
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
    </section>
  </div>;
}

