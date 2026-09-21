// Fetch the German Vosk model for voice scouting into static/models/ (git-ignored).
// Downloads the zip from alphacephei.com and repacks it as the .tar.gz that
// vosk-browser loads. Needs `tar` (Windows 10+, macOS, Linux) and, on Windows,
// PowerShell for the unzip.
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const NAME = 'vosk-model-small-de-0.15';
const out = join('static', 'models', NAME + '.tar.gz');
if (existsSync(out)) { console.log(out, 'already there'); process.exit(0); }
mkdirSync(join('static', 'models'), { recursive: true });
const work = join(tmpdir(), 'so-voice-model'); rmSync(work, { recursive: true, force: true }); mkdirSync(work, { recursive: true });
const zip = join(work, NAME + '.zip');
console.log('downloading', NAME, '(~46 MB) …');
const res = await fetch(`https://alphacephei.com/vosk/models/${NAME}.zip`);
if (!res.ok) throw new Error('download failed: ' + res.status);
writeFileSync(zip, Buffer.from(await res.arrayBuffer()));
if (process.platform === 'win32') execFileSync('powershell', ['-NoProfile', '-Command', `Expand-Archive -Force -Path '${zip}' -DestinationPath '${work}'`], { stdio: 'inherit' });
else execFileSync('unzip', ['-q', zip, '-d', work], { stdio: 'inherit' });
execFileSync('tar', ['-czf', out, '-C', work, NAME], { stdio: 'inherit' });
rmSync(work, { recursive: true, force: true });
console.log('wrote', out);
