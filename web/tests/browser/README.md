# Browser regression suites

Headless-Edge scripts driven over the DevTools protocol (Node 22+, native
WebSocket). They run against a throw-away server instance on port 8081 with
its own empty database, never against a running app:

```
SO_ADDR=127.0.0.1:8081 SO_DB=/tmp/so-test/test.db SO_DATA=/tmp/so-test SO_STATIC=../web/build ../server/target/debug/sideout-server
node seedho.mjs                 # coach, scout, viewer, roster, one match with lineup
node handover.mjs   <outdir>    # two devices: claim, banner, takeover, loss, offline queue, release, undo after completion
node handover2.mjs  <outdir>    # same-session tabs (Web Locks + fallback), takeover during an in-flight append, scout event with queued work
node handover3.mjs  <outdir>    # lineup editor under ownership, live → editor → live, unsent work blocks a lineup change, list/home hints
node guide.mjs      <outdir>    # the guide inside the app shell (/anleitung): top bar, tabs, theme, anchors, standalone /hilfe/ kept
node voice.mjs      <outdir> <wav>  # voice scouting: Edge's fake microphone plays a 48 kHz WAV of German commands (8 s lead, then "drei Angriff Punkt" …), actions land on /live/1
```

Each run needs a fresh database (the scripts drive matches to completion)
and writes screenshots into `<outdir>`. Edge is expected at the default
Windows install path; each script only kills the browsers it started.

The voice suite needs the model in `static/models` (`npm run voice-model`)
and a WAV. On Windows the German voice "Hedda" can synthesize one:
```
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.SelectVoice('Microsoft Hedda Desktop'); $s.Rate = -1
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(48000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile('cmds48.wav', $fmt); $pb = New-Object System.Speech.Synthesis.PromptBuilder
$pb.AppendBreak([TimeSpan]::FromSeconds(8))
foreach ($t in @('drei Angriff Punkt', 'vier Block Punkt', 'eins Zuspiel gut', 'und jetzt alle nach vorne', 'Fehler Gegner')) { $pb.AppendText($t); $pb.AppendBreak([TimeSpan]::FromSeconds(2.5)) }
$s.Speak($pb); $s.Dispose()
```
