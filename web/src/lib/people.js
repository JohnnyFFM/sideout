// Labels and helpers for people and roles, shared by the top bar, Konto and Teams.

export const ROLES = { coach: 'Trainer:in', assistant: 'Co-Trainer:in / Scout', viewer: 'Nur lesen' };
export const ROLE_SHORT = { coach: 'Trainer:in', assistant: 'Co-Trainer:in', viewer: 'Nur lesen' };

/** "Jonas Steitz" → "JS", "jonas" → "JO" */
export function initialsOf(name) {
  const words = String(name || '').trim().split(/\s+/).filter(Boolean);
  if (!words.length) return '??';
  const s = words.length >= 2 ? words[0][0] + words[1][0] : words[0].slice(0, 2);
  return s.toUpperCase();
}
