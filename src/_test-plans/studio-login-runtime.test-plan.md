# Test Plan: Studio login runtime

**Target:** `src/config.rs`, `src/runtime.rs`, `src/cli.rs`, and `src/gui.rs`
**Prepared:** 2026-08-25
**Status:** ACTIVE

## Behavior in scope

- A fresh launcher configuration chooses Studio's embedded WebView2 login path.
- Legacy browser-login configuration is parsed into one named login mode and is not rewritten as a second boolean authority.
- Every Studio launch receives one centrally owned WebView2 compatibility profile.
- The profile emulates Windows 8 for `msedgewebview2.exe` and selects WebView2's built-in SwiftShader renderer instead of Wine's hanging D3D11 WARP path.
- Before the first Wine helper starts, the prefix records X11 first when XWayland is available, restarts a stale session once if that choice changed, and retains Wayland as the no-X11 fallback.
- The managed Kombucha prefix supplies its CRT compatibility overrides, while the launcher installs the pinned DXVK files beside each selected Studio version.
- The explicit browser action remains a one-launch fallback and does not silently change later launches.

## Dependency tree

```text
StudioLoginMode (config leaf)
StudioRuntimePlan (runtime leaf)
configure_studio_environment -> StudioRuntimePlan, std::process::Command
configure_webview2_runtime -> StudioRuntimePlan, Wine process boundary
prepare_studio_runtime -> StudioRuntimePlan, WebView2 setup, DXVK setup, Wine process boundary
launch_latest_studio -> prepare_studio_runtime, optional browser watcher
LauncherApp settings/actions -> StudioLoginMode, launcher subprocess boundary
```

Unresolved imports, dynamic dispatch, and cycles:

- None. Wine, Roblox, Chrome, and the desktop protocol dispatcher are external process or platform boundaries.

## Dependency classification

| Dependency | Path or package | Classification | Planned treatment | Evidence |
|---|---|---|---|---|
| `StudioLoginMode` | `src/config.rs` | Internal leaf | Test directly and use the real enum in callers | Config load/save and CLI selection |
| `StudioRuntimePlan` | `src/runtime.rs` | Internal leaf | Test exact generated registry and environment values | Runtime command construction |
| Runtime command construction | `src/runtime.rs` | Internal node | Test with a real `std::process::Command` without spawning it | `configure_studio_environment` |
| CLI login selection | `src/cli.rs` | Internal node | Use the real config and runtime plan; no internal stubs | `BrowserLogin` and `Launch` branches |
| GUI selection | `src/gui.rs` | Internal node | Test the real command selection and status wording | action-button command vectors |
| Wine and registry | `/app/kombucha/bin/wine`, configured prefix | External effect | Do not invoke in unit tests; run a real installed-prefix proof after GREEN | `run_wine` and `reg.exe` |
| Roblox OAuth page and Chrome | Remote service and platform browser | External effect | Verify the host callback registration and perform one real human-authorized login proof | browser callback flow |

## Execution order

| # | Node | Role | Existing tests | Planned action |
|---|---|---|---|---|
| 1 | `StudioLoginMode` | Leaf | Partial | Extend for the embedded default, legacy migration, and saved representation |
| 2 | `StudioWebView2Compatibility` | Leaf | None | Add exact compatibility-contract tests |
| 3 | Runtime command construction | Composite | Partial | Verify environment and registry arguments through real builders |
| 4 | CLI login selection | Composite | None | Verify browser fallback does not persistently change login mode |
| 5 | GUI selection | Root | Partial | Verify the visible labels and commands match the corrected login path |

## Node plans

### 1. Studio login mode

**File:** `src/config.rs`
**Internal dependencies:** none
**External effects:** temporary filesystem paths already owned by the existing test suite

**Behaviors to prove:**

- Missing configuration selects embedded login.
- The legacy `embedded_webview` key maps to the matching named mode.
- New saves write one login-mode representation.

**Real Call-Site Inventory:**

| Caller | File | Type or shape passed | Constraints exercised |
|---|---|---|---|
| CLI configuration | `src/cli.rs` | `LauncherConfig` | persistent selection and one-shot browser fallback |
| GUI state | `src/gui.rs` | launcher login selection | visible selection and command generation |
| Runtime launch | `src/cli.rs` | selected mode | watcher versus embedded runtime |

**Fixture coverage:**

| Fixture | Constraint class covered | Matching caller |
|---|---|---|
| Empty config | default selection | fresh GUI and CLI launch |
| Legacy false config | migration boundary | existing user installations |
| Embedded mode config | saved canonical form | GUI and configure command |

- [x] Fixture set covers all real caller shapes.
- [x] No cheaper stand-in replaces a production config shape.

**TDD record:**

- [x] RED: the fresh-config expectation fails while browser login is still the default.
- [x] RED witnessed: `cargo test` exited 101; the fresh config produced `false` instead of embedded login.
- [x] GREEN: introduce the named mode, migrate legacy input, and save only the canonical form.
- [x] GREEN witnessed: config Behave scenarios pass.
- [x] REFACTOR: remove boolean login state from CLI and GUI callers.
- [x] REFACTOR witnessed: the source has one `StudioLoginMode` authority.

### 2. WebView2 compatibility profile

**File:** `src/runtime.rs`
**Internal dependencies:** none
**External effects:** none for the profile itself

**Behaviors to prove:**

- The profile uses `win8` for the WebView2 process.
- Browser arguments select `--use-angle=swiftshader` and do not disable every available paint path.
- DXVK and disabled Wine DLL overrides are emitted from the same profile used by Studio launches.

**Real Call-Site Inventory:**

| Caller | File | Type or shape passed | Constraints exercised |
|---|---|---|---|
| Registry setup | `src/runtime.rs` | profile-owned Windows version | WebView2 OS compatibility |
| Studio process setup | `src/runtime.rs` | profile-owned browser and DLL arguments | WebView2 rendering and CRT loading |

**Fixture coverage:**

| Fixture | Constraint class covered | Matching caller |
|---|---|---|
| Real compatibility profile | all closed profile values | both production callers |

- [x] Fixture set covers all real caller shapes.
- [x] The profile has no alternate constructor or caller-provided values.

**TDD record:**

- [x] RED: the renderer assertion rejects the no-paint `--disable-gpu --disable-software-rasterizer` profile.
- [x] RED witnessed: `cargo test rendering_studio_s_embedded_login_page` exited 101 and reported the old arguments.
- [x] GREEN: correct the single profile and route both writers through it.
- [x] GREEN witnessed: the compatibility-profile Behave scenario passes.
- [x] REFACTOR: remove duplicate literal settings and sweep for bypasses.
- [x] REFACTOR witnessed: both Studio launch paths call `configure_studio_environment`.

### 3. Runtime command construction

**File:** `src/runtime.rs`
**Internal dependencies:** nodes 1 and 2
**External effects:** `std::process::Command` is constructed but not spawned

**Behaviors to prove:**

- A real Studio command receives the profile's browser arguments and DLL overrides.
- The generated registry command receives the profile's Windows version.

**Real Call-Site Inventory:**

| Caller | File | Type or shape passed | Constraints exercised |
|---|---|---|---|
| `run_studio` | `src/runtime.rs` | Studio command | normal launch |
| `run_studio_auth` | `src/runtime.rs` | callback command | browser fallback callback |
| WebView2 setup | `src/cli.rs` | Wine path and prefix | prefix registration |

**Fixture coverage:**

| Fixture | Constraint class covered | Matching caller |
|---|---|---|
| Unspawned real command | environment mutation | both Studio launch paths |
| Generated registry arguments | registry mutation | setup path |

- [x] Fixture set covers all real caller shapes.
- [x] No internal function is stubbed.

**TDD record:**

- [x] RED: generated values expose the current incompatible profile.
- [x] RED witnessed: `cargo test` exited 101 with both incompatible generated values.
- [x] GREEN: command builders consume the corrected profile.
- [x] GREEN witnessed: generated environment assertions pass.
- [x] REFACTOR: none beyond deleting old literals.
- [x] REFACTOR witnessed: `cargo clippy --all-targets --all-features -- -D warnings` passes.

## Live failure and proof record

- Baseline: WebView2 loaded Roblox login with HTTP 200, then its WARP GPU helper hung and Studio reported `LoginDialog Error` at 103.878 seconds.
- Reference-DXVK control: adding DXVK alone reproduced the same failure at 103.790 seconds, ruling DXVK out as the login fix.
- Failed control: `--disable-gpu --disable-software-rasterizer` remained alive but a direct 500×712 X11 capture was completely white, proving that DOM load was not visual success.
- Corrected profile: `--use-angle=swiftshader` rendered the complete Roblox login form, remained alive beyond the old 103-second failure boundary, and still contained 56,137 visible colors at 138 seconds with no WebView error or hang in the Studio log.
- Unit proof: 30 Behave tests pass, including the exact WebView arguments, responsive GUI width, one-command-only GUI launch, host callback registration, X11/Wayland selection, and managed DXVK installation beside a new Studio version.

### 4. CLI login selection

**File:** `src/cli.rs`
**Internal dependencies:** nodes 1 through 3
**External effects:** Wine process and browser watcher, covered by installed integration proof

**Behaviors to prove:**

- `launch` uses the saved named mode.
- `browser-login` chooses one external-browser launch without saving that mode.
- Browser mode verifies the host callback and reports watcher failure to the GUI instead of detaching and discarding it.

**Real Call-Site Inventory:**

| Caller | File | Type or shape passed | Constraints exercised |
|---|---|---|---|
| CLI parser | `src/cli.rs` | command tokens | persistent versus one-shot selection |
| GUI process runner | `src/gui.rs` | command vector | browser fallback button |

**Fixture coverage:**

| Fixture | Constraint class covered | Matching caller |
|---|---|---|
| Parsed launch command | saved mode | CLI and GUI launch |
| Parsed browser fallback | one-shot override | GUI fallback action |

- [x] Fixture set covers all real caller shapes.
- [x] No process boundary is silently stubbed.

**TDD record:**

- [x] RED: live browser fallback reached the redirect page without a verified return handler.
- [x] RED witnessed: the redirect remained open and Studio stayed unauthenticated.
- [x] GREEN: register and query the host MIME handler before browser mode, then wait synchronously for the authorization page handoff.
- [x] GREEN witnessed: the Flatpak command-shape Behave scenario and host `xdg-mime`/`gio mime` query identify the exported callback entry.
- [x] REFACTOR: remove the detached `browser-login-watch` subprocess and its discarded output.
- [x] REFACTOR witnessed: browser handoff success or failure now returns through the original GUI command.

### 5. GUI login selection

**File:** `src/gui.rs`
**Internal dependencies:** nodes 1 and 4
**External effects:** launcher subprocess and human authorization

**Behaviors to prove:**

- The main Studio copy describes embedded login as the managed default.
- Browser login is presented as recovery, not the normal route.

**Real Call-Site Inventory:**

| Caller | File | Type or shape passed | Constraints exercised |
|---|---|---|---|
| Egui action button | `src/gui.rs` | launcher command vector | one-shot fallback |
| Egui settings control | `src/gui.rs` | named mode | persistent mode selection |

**Fixture coverage:**

| Fixture | Constraint class covered | Matching caller |
|---|---|---|
| Real command vector and status copy | visible behavior | both GUI callers |

- [x] Fixture set covers all real caller shapes.
- [x] No UI framework behavior is replaced.

**TDD record:**

- [x] RED: the old installed GUI described browser sign-in as the default and sent duplicated command tokens.
- [x] RED witnessed: the old package text and command construction contained the stale behavior.
- [x] GREEN: describe in-Studio sign-in as the default and build one exact launcher argument list per click.
- [x] GREEN witnessed: the GUI Behave scenario proves `--config`, its path, and `launch` occur exactly once.
- [x] REFACTOR: use content-sized buttons that wrap and keep advanced details collapsed.
- [x] REFACTOR witnessed: formatter and strict Clippy checks pass.

## Stub justification log

No unit-test stubs are planned. Wine, Roblox, Chrome, and desktop dispatch are tested as real external boundaries after the pure regression suite is green.

## Ambiguities and user decisions

| Item | What blocks a safe classification | Options surfaced | Decision |
|---|---|---|---|
| Human account authorization | Only the account owner can approve the Studio grant | User clicks Continue once after the rebuilt launcher reaches the account screen | Required human step |

## Phase gates

- [x] Dependency tree built: every internal node and external boundary recorded.
- [x] Plan reviewed: boundary list and absence of internal stubs surfaced in the active task.
- [ ] Tests drafted: RED completed for every changed behavior.
- [ ] Tests run: GREEN completed bottom-up with real internal dependencies.
- [ ] Coverage verified: REFACTOR complete, every node covered, and real login proof attempted.

## Plan change log

| Date | Change | New evidence | User notified? |
|---|---|---|---|
| 2026-08-25 | Replaced browser-callback patching with a managed embedded-login runtime fix | Claude Code and Vinegar both place the remaining failure before Linux callback dispatch; Vinegar's current profile differs at the WebView2 compatibility owner | yes |

## Completion proof

- Test commands and exit codes: pending
- Nodes with their own tests: pending
- Approved external stubs: none
- Stub audit: pending
- Fixture realism audit: pending
- Remaining blocks or explicit waivers: one human Continue action for live OAuth proof
