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

/** hand the viewer a JSON file (exports) */
export function downloadJson(name, obj) {
  const url = URL.createObjectURL(new Blob([JSON.stringify(obj, null, 1)], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
}
