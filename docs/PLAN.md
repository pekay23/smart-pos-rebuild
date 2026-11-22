# Smart POS Server 1.5.0 reconstruction plan

> Product-wide continuation checklist: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). This plan tracks the separate original-binary reference reconstruction; its parity work remains incomplete.

## Goal and evidence standard

Reconstruct the executable as an editable Windows project under this directory. Treat the original PE, its extracted resources, and the Ghidra decompilation as evidence. Record which behavior is confirmed, inferred, or still unknown. A visual or behavioral 1:1 claim requires side-by-side execution and comparison; source resemblance alone is not proof.

The supplied binary is a 32-bit Windows executable (180,313,870 bytes). The decompiled entry point at `0x40338f` implements bootstrap/installer work before the main UI. The reference export currently contains 104 function files, not the 269 stated in the handoff. Establish the actual resource and function inventory before implementing feature claims.

## Phases

1. **Evidence and build foundation** — inventory the PE, resources, imports, and available decompilation; preserve original resource payloads and hashes; make the CMake project correctly target 32-bit Windows and compile under MSVC or MinGW.
2. **Startup and lifecycle** — match argument parsing, common controls, OLE initialization, temp-path fallback, installer mode, error paths, cleanup, and shutdown behavior from `0x40338f` and its callees.
3. **Resource and screen recovery** — extract all icons, dialogs, strings, menus, and version data; decode dialog templates into editable resources; map each screen’s controls and transitions to observed message handlers.
4. **Core message router** — recover the dispatch contract at `0x401434` (message arguments, buffer indexing, return values, side effects); implement handlers in small modules and link them through a single router.
5. **Subsystems** — implement file/path, registry/config, process/handle, data transfer, UI/control, INI, crypto/hash, strings, and error handling only where the decompilation establishes semantics. Keep irreversible host actions behind explicit UI actions during development.
6. **Application screens** — recover the main dialog procedure and screen transitions; preserve original labels, sizing, styles, and control IDs from resources rather than inventing substitute POS workflows.
7. **Parity verification** — build x86 Debug and Release; compare resource inventories, screen captures, command-line modes, message results, filesystem/registry effects, and error behavior against the original in a disposable Windows VM. Maintain a parity matrix with evidence for each item.
8. **Review package** — keep source, extracted resources, build output, and verification notes together in this directory; document build/run instructions and unresolved differences.

## Current status

- [x] Initial project and source files inspected.
- [x] Target path and SHA-256 recorded; PE inventory completed.
- [x] Ghidra reference directory found; inventory says 104 function files.
- [x] Correct x86 CMake configuration and compile baseline.
- [x] Extract all 22 PE resources; exact-check all 18 dialog templates in the rebuilt PE.
- [ ] Recover dialog/message contracts from complete decompilation.
- [ ] Implement the installer runtime and verify screens and behavior screen by screen.

## Parity limits

The current C files are a scaffold, not a complete reconstruction. The original `src/window_main.c` stub was address-mislabeled and read a hard-coded original-process address; that invalid stub has been replaced by a dialog-template host. The reconstructed app remains incomplete. Do not describe it as functionally equivalent until every required phase is complete and the comparison matrix is supported by runs of both programs.

## Verified progress (2026-10-29)

- Target SHA-256: `1c8efed1ac2bc85df3260f1ce0c4e9a51ad8fc3bb30ca2c227de759fca39b252`.
- PE machine: x86 (`0x014c`); total size 180,313,870 bytes.
- PE overlay starts at byte 48,128 and is 180,265,742 bytes. Its signature includes `NullsoftInst`; this is a compressed NSIS installer payload and is not reproduced by the current project.
- Original resources: 22 total, including 18 RT_DIALOG templates, one RT_ICON image, one icon group, version metadata, and a manifest.
- Current Release build: succeeds with MSVC 19.44 x86; output is `build-nmake/app/smart-pos-server-rebuild.exe`.
- `build_x86.cmd` configures and builds the x86 app at `build-msvc-x86/app/smart-pos-server-rebuild.exe`.
- `tools/verify_dialog_resources.py` confirms 18/18 dialog template byte sequences, the icon/group icon, and the manifest match the original; version strings also match. The rebuild is not functionally equivalent: it currently launches one recovered dialog and does not implement the NSIS runtime, the 68-case command dispatcher, or the 180 MB payload.
- The app accepts `/screen <resource-id>` to select one of the recovered dialog templates for inspection (IDs: 105, 106, 111, 205, 206, 211, 305, 306, 311, 405, 406, 411, 505, 506, 511, 605, 606, 611). This is a review aid, not original installer behavior.
