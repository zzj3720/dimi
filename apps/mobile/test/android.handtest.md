# k-3720 Android client handtest

## Preconditions

- An arm64 Android 8+ phone or emulator with the internal release APK installed
- A public TLS WebSocket endpoint forwarding to `@dimi-agent/relay`
- A local server and `vp run dev:cli -- remote` bridge from the build under test
- A model and tool environment that can complete a prompt, request approval, and call `AskUserQuestion`

## Pair and resume

1. Start the relay, server, and bridge.
2. Scan the bridge QR code in the Android app.
3. Confirm the app shows the paired runtime as **Online** and lists existing sessions.
4. Force-stop and reopen the app.
5. Confirm the same runtime reconnects without pairing again.
6. Stop and restart the bridge while the app remains open.
7. Confirm the app transitions **Offline → Online**, reloads the selected session, and does not duplicate transcript items.
8. Trigger a server `resync_required` and confirm the same reload behavior.

## Prompt, steer, and cancel

1. Open a session and send a prompt that runs a tool for at least 20 seconds.
2. Confirm the UI shows only a typing/activity hint while the turn is active.
3. Confirm no token-level partial Markdown appears; the assistant message appears only as a stable transcript update.
4. Confirm a subagent transcript update does not replace or append to the main-agent timeline.
5. Queue a steer message and confirm the next model response follows it.
6. Start another long-running turn and tap cancel.
7. Confirm the turn reaches the cancelled state and the app becomes idle.

## Approval

1. Send a prompt that requires a tool approval.
2. Confirm the approval card identifies the action and shows a bounded, readable `tool_input_display` preview.
3. Confirm raw secrets and unbounded tool input are not displayed.
4. Enter feedback and approve once. Confirm the exact feedback and `once` scope reach the pending approval.
5. Repeat and approve for the session. Confirm the `session` scope reaches the pending approval.
6. Repeat and reject. Confirm the tool does not run.
7. Confirm every resolved approval card disappears and a repeated tap cannot submit it twice.

## Question and reconnect

1. Ask the agent to call `AskUserQuestion` with descriptions, two options, and `allow_other`.
2. Wait until the owning turn ends and confirm the question remains pending.
3. Stop the bridge and confirm the selected session shows **Offline** while retaining the question card.
4. Restart the same bridge and confirm the app returns **Online** with the same pending question.
5. Select the second option and submit. Confirm the option id, not its display text, is sent.
6. Confirm the question becomes answered, its background tool completes, and the card disappears.
7. Repeat with **Other**, enter free text, and confirm the answer uses the structured free-text form.
8. Repeat with a multiple-choice question and confirm selected options plus Other are preserved.
9. Create two pending questions. Confirm only the first is actionable; after resolving it, the second appears.
10. Dismiss a dismissible question and confirm it disappears without a fabricated answer.

## Transcript presentation

1. Confirm consecutive tool calls appear as one `Used n tools` row.
2. Expand it and confirm the existing per-tool fold still works.
3. Render Markdown containing a long code block.
4. Confirm the transcript shows a compact code preview and tapping it opens the dedicated code screen.
5. Long-press code on the dedicated screen and confirm Android native copy works.
6. Render Markdown containing an external `https://` image.
7. Confirm the app does not automatically fetch or render the external image.
8. Confirm the transcript does not expose raw base64 media or unbounded tool output.

## Remote capability boundary

1. From a paired test client, attempt the encrypted HTTP routes for terminal creation, runtime status/meta, and transcript mutation.
2. Confirm the bridge rejects each request and Kap receives none of them.
3. Attempt `watch_fs_add`, `terminal_input`, a `subscribe` frame smuggling a `watch_fs` field, and `subscribe_v2` with transcript grade `delta`.
4. Confirm the bridge drops each frame and Kap receives none of them.
5. Confirm the relay logs or captures contain no plaintext prompt, local bearer token, approval input, or answer.
6. Install a release build and confirm a non-TLS `ws://` pairing URI is rejected. A debug build may accept it only for the local handtest.

## Release and Android platform

1. Build the arm64 internal release APK from the exact commit under test with package `org.k3720.mobile.internal`, a stable private signing key, and version code 1. Record its SHA-256 and signing certificate digest.
2. Install it with `adb install`; do not use Expo Go as release evidence.
3. Pair it with the public test relay `wss://relay.k.test.3720.org` and confirm it reconnects after a force-stop.
4. Confirm Android settings show the camera permission and the internal build's install-packages capability. Confirm external storage, microphone, overlay, and vibration permissions are absent.
5. Build a higher version code with the same package and signing key, publish it through the Distribution Worker, and confirm the immutable APK URL and `latest.json` have matching size and SHA-256.
6. Confirm the installed version automatically detects and downloads the newer build, then shows **Install** only after verification. Installing the initial build with `adb` is bootstrap setup, not upgrade evidence.
7. Tap **Install**. If Android asks to trust this app as an install source, grant only that setting; then confirm the package installer.
8. Confirm Android reports version code 2, the previously paired runtime remains present, and the app reconnects without pairing again.
9. Force-stop the app during an active turn, reopen it, and confirm the stable transcript and pending interaction reload.

## Distribution page

1. Open `https://install.k.test.3720.org` at widths 320, 375, 414, and 768 pixels.
2. Confirm the page shows the published version, version code, date, commit, SHA-256, and APK size from the live manifest.
3. Scan the QR code and confirm it resolves to the same install host, not directly to a mutable APK URL.
4. Download the APK and confirm its response type is `application/vnd.android.package-archive` with immutable caching.
5. Confirm `https://install.k.test.3720.org/android/internal/latest.json` returns `Cache-Control: no-store`.

## Explicitly not claimed by this handtest

- Attachment upload/download is not shipped until the encrypted bounded binary protocol exists.
- Session and transcript pagination beyond the first 100 items is not shipped yet.
- Background completion notifications are not shipped until Android has a reliable background or push design.
- Model/provider, terminal, host filesystem, plugin, skill, MCP, OAuth, and runtime-management controls remain host-side.

## Evidence to record

- APK filename, SHA-256, package name, and version
- Android version code, architecture, signing certificate digest, previous/new installed version, bootstrap method, and retained pairing identity
- Distribution manifest, immutable APK URL, and install-page screenshots at 320/375/414/768 pixels
- Runtime and relay endpoints without credentials
- Screenshots for paired, reconnect, approval, working, cancelled, pending question, reconnected question, and answered question states
- Authoritative session events for cancel and question resolution
- Bridge-side evidence that rejected routes/frames never reached Kap
- Android permission listing and release-build `ws://` rejection
