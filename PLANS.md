# mesh-audio Plans

## Goal
Make `mesh-audio` a practical multi-speaker sync controller with clear UX, stable Bluetooth behavior, and optional VB-Cable workflow.

## Current State (Done)
- Added 3-tab structure: `Devices`, `VB-Cable`, `Sync`.
- `VB-Cable` tab moved to second position.
- `Sync` tab works independently from VB-Cable.
- Added signed per-device offsets and global delay model in backend.
- Added several sync presets in `VB-Cable` tab.
- Added `Reset All Delays` in `VB-Cable` tab.
- Added in-app VB-Cable install/check commands:
  - `check_vbcable_installed`
  - `install_vbcable` (requires UAC + reboot)
- Added concise bilingual `README.md`.

## Known Limitations
- Perfect sync with system default output is physically limited in default loopback mode.
- VB-Cable install still depends on admin consent and reboot (Windows driver policy).
- No rich runtime health diagnostics in UI yet.
- No persistent room/profile storage in backend yet.

## Priority Roadmap

### P0 - Stability and Usability (next)
1. Add robust `Apply all offsets` / `Reset all offsets` parity in both `Sync` and `VB-Cable` tabs.
2. Add explicit user feedback for long operations:
   - installing VB-Cable,
   - applying profile,
   - resync.
3. Add post-install wizard state in `VB-Cable` tab:
   - installed,
   - reboot required,
   - default route check.

Acceptance:
- User can complete setup without external notes.
- Every long action has visible status + error path.

### P1 - Route Check and Health Metrics
1. Add backend command: route health snapshot.
2. Expose runtime metrics per target:
   - queue depth estimate,
   - underrun count,
   - correction activity.
3. Add `VB-Cable` tab health panel with traffic-light statuses.

Acceptance:
- User can see which speaker drifts/drops and why.
- UI shows actionable hints (increase buffer, reduce aggressiveness, etc.).

### P2 - Profiles and Recovery
1. Add persistent room profiles (JSON):
   - selected targets,
   - global delay,
   - warmup/ring,
   - offsets,
   - auto-adjust mode.
2. Add recovery actions:
   - re-init mirror worker,
   - soft fallback profile,
   - emergency stop + restore.

Acceptance:
- One-click load for common rooms/scenarios.
- Recovery works without app restart.

### P3 - VB-Cable First-Class Mode
1. Add dedicated mode flag in backend: `DefaultLoopback` vs `VBCableRoute`.
2. Validate and surface source identity clearly.
3. Tune presets specifically per mode.

Acceptance:
- Behavior is predictable and explained by selected mode.
- No mixed assumptions between tabs.

## Suggested Preset Set
- Music Room
- Movie / TV
- Low Latency
- BT Stable
- Front Near / Rear Far
- Party (Max Stability)
- Speech / Podcast
- Gaming (Balanced)

## Technical Debt Queue
- Clean warnings in `audio.rs`, `main.rs`, `app.rs`.
- Normalize naming (snake_case warnings).
- Refactor large `audio.rs` into modules:
  - device discovery,
  - mirror worker,
  - sync controller,
  - diagnostics.

## Release Checklist
1. `npm run build`
2. `cargo check`
3. `cargo build --release`
4. Manual smoke test:
   - device selection,
   - start/stop mirror,
   - profile apply,
   - reset offsets,
   - VB install/check flow.

## Notes
- Driver installation cannot bypass UAC.
- Reboot reminder must remain explicit after VB-Cable install.
