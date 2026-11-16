import type { WorkStatus } from './types';

// GJC 0.10.2 has no host event. Match the interactive chrome at the
// live end of the buffer. Never search the conversation scrollback.
export function legacyGjcStatus(lines: string[], previous?: WorkStatus): WorkStatus | undefined {
  if ((previous === 'in_progress' || previous === 'waiting_for_reply') &&
      lines.some(line => /^up\/down navigate\s+enter select\s+(?:←\/→ question\s+)?esc cancel$/i.test(line.trim()))) {
    return 'waiting_for_reply';
  }
  const last = (pattern: RegExp) => {
    for (let i = lines.length - 1; i >= 0; i--) if (pattern.test(lines[i].trim())) return i;
    return -1;
  };
  const composer = last(/^╭[─]+╮$/);
  const footer = last(/\bv0\.10\.2$/);
  if (composer < 0 || footer < 0 || Math.abs(composer - footer) > 8) return previous;
  if (lines.slice(Math.max(0, footer - 7), composer).some(line =>
    /^\s*(?:[\u2800-\u28ff]\s+)?(?:Working…|Thinking…)(?:\s|$)/u.test(line))) {
    return 'in_progress';
  }
  if (previous === 'in_progress' || previous === 'waiting_for_reply') return 'completed';
  return previous;
}
