import type { Agent } from './types';

const arrows: Record<string, string> = { ArrowUp: 'A', ArrowDown: 'B', ArrowRight: 'C', ArrowLeft: 'D' };

export function harnessKey(event: KeyboardEvent, agent: Agent | undefined): string | undefined {
  if (event.type !== 'keydown' || event.isComposing || event.metaKey || event.getModifierState('AltGraph')) return;
  const arrow = arrows[event.key];
  if (arrow && event.altKey && !event.ctrlKey && !event.shiftKey) {
    // xterm.js 5 maps unshifted Alt+arrows to Ctrl+arrows on Windows.
    return `\x1b[1;3${arrow}`;
  }
  if (event.key !== 'Enter' || event.altKey) return;
  // These harnesses distinguish modified Enter from submit. Generic
  // terminals keep xterm's standard input handling.
  if (agent === 'gajae' && (event.shiftKey || event.ctrlKey)) {
    return `\x1b[13;${1 + Number(event.shiftKey) + 4 * Number(event.ctrlKey)}u`;
  }
  if ((agent === 'claude' || agent === 'hermes') && (event.shiftKey || event.ctrlKey)) return '\n';
}
