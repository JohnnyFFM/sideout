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
```

Each run needs a fresh database (the scripts drive matches to completion)
and writes screenshots into `<outdir>`. Edge is expected at the default
Windows install path; each script only kills the browsers it started.
