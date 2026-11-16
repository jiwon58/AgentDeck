import { useEffect, useRef, useState, type RefObject } from 'react';
import { SearchAddon, type ISearchOptions } from '@xterm/addon-search';
import type { IMarker, Terminal } from '@xterm/xterm';

const highlights: ISearchOptions['decorations'] = {
  matchBackground: '#634d2c', matchOverviewRuler: '#bd9860',
  activeMatchBackground: '#315778', activeMatchBorder: '#83cbf7', activeMatchColorOverviewRuler: '#83cbf7',
};
const highlightLimit = 1000;

export function TerminalTools({ terminalRef, name, comparison, readOnly, toggleReadOnly }: {
  terminalRef: RefObject<Terminal | null>;
  name: string;
  comparison: boolean;
  readOnly: boolean;
  toggleReadOnly: () => void;
}) {
  const addon = useRef<SearchAddon | null>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState({ resultIndex: -1, resultCount: 0 });
  const [found, setFound] = useState(false);
  const [markers, setMarkers] = useState<{ marker: IMarker; number: number }[]>([]);
  const nextBookmark = useRef(1);

  useEffect(() => {
    if (searchOpen) searchInput.current?.focus();
  }, [searchOpen]);

  const search = (text: string, direction: 'next' | 'previous', incremental = false) => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    if (!addon.current) {
      const searchAddon = new SearchAddon({ highlightLimit });
      terminal.loadAddon(searchAddon);
      searchAddon.onDidChangeResults(setResults);
      addon.current = searchAddon;
    }
    if (!text) {
      addon.current.clearDecorations();
      setResults({ resultIndex: -1, resultCount: 0 });
      setFound(false);
      return;
    }
    const options = { decorations: highlights, incremental };
    setFound(direction === 'next' ? addon.current.findNext(text, options) : addon.current.findPrevious(text, options));
  };
  const closeSearch = () => {
    addon.current?.clearDecorations();
    setSearchOpen(false);
    setQuery('');
    terminalRef.current?.focus();
  };
  const bookmark = () => {
    const terminal = terminalRef.current;
    if (!terminal || terminal.buffer.active.type !== 'normal' || markers.length >= 50) return;
    const { baseY, cursorY, viewportY } = terminal.buffer.active;
    const line = terminal.getSelectionPosition()?.start.y ?? viewportY;
    if (markers.some(item => item.marker.line === line)) return;
    const marker = terminal.registerMarker(line - baseY - cursorY);
    if (!marker) return;
    const number = nextBookmark.current++;
    marker.onDispose(() => setMarkers(current => current.filter(item => item.marker !== marker)));
    setMarkers(current => [...current, { marker, number }]);
  };
  const showSearch = () => searchOpen ? searchInput.current?.focus() : setSearchOpen(true);
  return <div className="terminal-tools" data-deck-shortcuts onKeyDown={event => {
    if (event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey && event.key.toLowerCase() === 'f'
        && !(event.target instanceof HTMLInputElement)) {
      event.preventDefault();
      showSearch();
    }
  }}>
    <div className="terminal-tool-row">
      {comparison && <strong className="ellipsis grow" title={name}>{name}</strong>}
      <button type="button" aria-expanded={searchOpen} onClick={showSearch}>Find</button>
      <button type="button" title="Bookmark the selection or the first visible line" onClick={bookmark}>Bookmark</button>
      {markers.length > 0 && <details className="terminal-bookmarks">
        <summary>Bookmarks ({markers.length})</summary>
        <div className="bookmark-list">
          {markers.map(({ marker, number }) => <div className="row" key={marker.id}>
            <button type="button" onClick={() => {
              const terminal = terminalRef.current;
              if (terminal?.buffer.active.type === 'normal' && !marker.isDisposed) terminal.scrollToLine(marker.line);
            }}>Bookmark {number}</button>
            <button type="button" aria-label={`Remove bookmark ${number}`} onClick={() => marker.dispose()}>×</button>
          </div>)}
        </div>
      </details>}
      <button type="button" onClick={() => terminalRef.current?.scrollToBottom()}>Latest</button>
      <button type="button" aria-pressed={readOnly} onClick={toggleReadOnly}>Read-only</button>
    </div>
    {searchOpen && <form className="terminal-find" onSubmit={event => {
      event.preventDefault();
      search(query, 'next');
    }}>
      <input ref={searchInput} aria-label="Find in terminal output" placeholder="Find in output" value={query}
        onChange={event => { setQuery(event.target.value); search(event.target.value, 'next', true); }}
        onKeyDown={event => {
          if (event.key === 'Escape') { event.preventDefault(); closeSearch(); }
          if (event.key === 'Enter' && event.shiftKey) { event.preventDefault(); search(query, 'previous'); }
        }} />
      <output aria-live="polite">{query && (results.resultCount >= highlightLimit
        ? `${highlightLimit}+ matches`
        : results.resultCount ? results.resultIndex < 0
          ? `${results.resultCount} matches` : `${results.resultIndex + 1} / ${results.resultCount}`
          : found ? 'Match found' : 'No matches')}</output>
      <button type="button" disabled={!query} aria-label="Previous match" onClick={() => search(query, 'previous')}>↑</button>
      <button type="submit" disabled={!query} aria-label="Next match">↓</button>
      <button type="button" aria-label="Close terminal search" onClick={closeSearch}>×</button>
    </form>}
  </div>;
}
