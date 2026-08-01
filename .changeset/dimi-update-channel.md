---
"@dimi-agent/cli": patch
---

Enable Dimi's own update channel backed by GitHub Releases. `dimi upgrade` and the startup check now read `latest.json` from the newest GitHub Release (published automatically by the `publish-update-channel` workflow on `v*` tags), and source builds get a `git pull --ff-only && vp install` manual upgrade hint instead of pointing at the pre-fork npm package.
