## fork changes (adkala/herdr)

This fork carries the changes below on `master`. Each is meant to stay
cherry-pickable onto the current `upstream/master`
([ogulcancelik/herdr](https://github.com/ogulcancelik/herdr)), so it can become an
upstream PR without untangling anything. Staged branches are pushed to `origin`
(this fork), so a fresh clone has them.

| change | commit on `master` | staged branch | notes |
| --- | --- | --- | --- |
| `advanced.escape_time_ms`: configurable lone-Escape flush delay, like tmux `escape-time` (default unset — 10ms, or 150ms while mouse capture holds a pending Escape) | `5e218ec`, `56b4a8d` (thin-client fix), `03c878a` (docs) | `pr/escape-time-ms` | PR candidate. The first commit alone was inert: it wired the key only into the in-process reader, while the thin client hardcoded its flush window. Since the 2026-09-12 rebuild only the thin-client reader exists (upstream #3487 removed the in-process `--no-session` path), so `5e218ec` now carries just the config key and `56b4a8d` the behavior. The branch squashes all three into one cherry-pickable commit |
| `advanced.osc52_paste`: opt-in OSC 52 paste support — answers `OSC 52 ; c ; ?` clipboard read queries (off by default). `true`/`"server"` replies with the server machine's clipboard; `"terminal"` forwards the query to the local terminal so panes paste from the local clipboard over ssh | `93fc59d`, `e16fdfe`, `d2fc529` (e2e test), `d66b789` (docs) | — | PR candidate; lives on `master` only. Since the 2026-09-12 rebuild the `"terminal"` mode rides the client-shell lane: the server sends `ServerMessage::ClipboardQuery` and the shell relays its terminal's answer as `ClientMessage::ClientShellHostClipboardReply` (both appended after `EndpointControl`, wire tag 21); direct `pane attach` clients are not asked. Upstream now treats those enums as append-closed, so an upstream PR should carry the query and reply as `EndpointControl` kinds instead of new variants |
| manual artifact builds stamp `HERDR_BUILD_CHANNEL=dev` + `HERDR_BUILD_ID=<short sha>`, so binaries report `herdr <version>-dev.<sha>` | `ee321cc` | — | fork-only build identity. The workflow otherwise follows upstream, including the Zig 0.16.0 setup-zig step |
| `ui.focused_pane_border` / `ui.unfocused_pane_border`: separate focused-pane border colors like tmux `pane-active-border-style` / `pane-border-style` | `c255d3f` | `pr/focused-pane-styles` | PR candidate. The original `ui.dim_unfocused_panes` toggle came off in the 2026-09-12 rebuild (see below) |
| `ui.tab_titles = "terminal_title"`: auto-named tabs inherit their focused pane's terminal title, like tmux automatic-rename (default stays numbered tabs) | `599f43f` | `pr/auto-tab-titles` | PR candidate. Since upstream #3487 the client shell draws whatever tab label the server projects, so the inherited title reaches the tab strip, mobile header, navigator and window title through the shell snapshot; a title change forces a snapshot refresh the same way sidebar title tokens do |
| attached clients render colored underlines: the wire `CellData` now carries the SGR 58 underline color and the client emits `58:2::r:g:b` / `58:5:n`, so Neovim's red diagnostic undercurls stay red through herdr (they fell back to the text color). Bumps the protocol to 23 (upstream 0.9.0 shipped 22) | `628ecad` | `pr/underline-color` | PR candidate; upstream bug (#1252, #1169, #1178 are still open), fork carries it until merged. Restart the server after installing (protocol bump). The extra `CellData` field also changes the `shell.surface.v1` cell encoding, so a fork client and an upstream server (or vice versa) must not be mixed. Upstream's CLAUDE.md now freezes generation-1 codecs (two bincode digests were re-blessed here), so an upstream PR would have to introduce a new surface codec instead of widening `CellData` |
| custom popup keybinds take their border title from `description` instead of always rendering the literal `popup` | `a1d06ba` | `pr/popup-title-from-description` | PR candidate; upstream has no other way to name a popup — `popup_pane` sits outside any workspace so `pane.rename` cannot reach it, and `$HERDR_PANE_ID` is unset inside one. Lives in `src/app/custom_commands.rs` since the 2026-09-12 rebuild |
| mobile layout marks the zoomed tab: the header's `tab N · i/n` label and the switcher's per-space detail row carry the desktop tab bar's ` Z` suffix, since mobile draws no tab strip or `tab_bar_right` status area | `931ca6f` | `pr/mobile-zoom-indicator` | PR candidate; lives in `src/client/shell/mobile.rs` since the 2026-09-12 rebuild |
| sidebar agent rows: the `tab` token shows the title an auto-named tab inherits under `ui.tab_titles = "terminal_title"` (stock only knows custom names and numbers there), and an inherited title fills it even in a single-tab workspace, so `rows = [["state_icon", "workspace"], ["agent", "tab"]]` reads `claude · <title>` | `3c4aac9` | `pr/sidebar-tab-label` | PR candidate; folds into `pr/auto-tab-titles` if that goes upstream. The client shell decides token visibility, so each snapshot tab now carries a serde-defaulted `inherited_label` flag (older snapshots still decode) |
| `distribution/install.sh` installs the fork's Linux dev build from the `adkala/herdr` `dev` release instead of the upstream `herdr.dev` manifest (macOS still uses the manifest); checksum is skipped on that path because the `dev` release ships no sha256 manifest | `b641024` | — | **fork-only, never upstream**; points the installer at the fork's own binary so `curl … install.sh \| sh` on Linux gets the dev channel, not upstream stable. Upstream moved the script from `website/` to `distribution/` when the website left this repo |

### staged, not on `master`

Reverted from `master` on 2026-07-27: each needs more work before it ships on the
`dev` channel. Every one is intact on the branch below — nothing was discarded.
`master` was most recently rebuilt on upstream `d184b41` (2026-09-12); the tip
before that rebuild is kept at `backup/master-9931b02`. Earlier rebuilds are at
`backup/master-f65c269` (onto `1c76079`, 2026-08-21), `backup/master-54fe477`
(onto `d76657f`, 2026-08-14) and `backup/master-0aed437` (onto `952729e`,
2026-08-13), and the 2026-07-27 pre-revert tip is at `backup/master-f2facce`.
The 2026-09-12 rebuild crossed upstream's client-shell refactor (#3487: the TUI
now renders in each client and `--no-session` is gone), the 0.9.0 release and
the Zig 0.16.0 / libghostty upgrade (#3906), so the tab-title, mobile-zoom,
sidebar-token and popup-title patches were ported into `src/client/shell/` and
`src/app/custom_commands.rs` instead of carried unchanged, the OSC 52 terminal
mode got its own client-shell message, and three things came off: the
`ui.dim_unfocused_panes` toggle, the macOS `zig@0.15` build step, and
`pr/pane-bin-path` (all listed below). Building now needs Zig 0.16.0 (`ZIG=…`
or `brew install zig`).
`backup/master-65408bd` keeps the tip before the 2026-08-22 cleanup that fixed
the OSC 52 e2e test's wire variant index and reworded two commit subjects that
failed upstream's conventional-commits check.

To reinstate one, cherry-pick its branch onto `master` and move its row up.

| change | branch | why it came off |
| --- | --- | --- |
| ~~`ui.sidebar_worktree_connectors`~~ | *(branch deleted 2026-08-05)* | dropped: upstream #1873's tree connectors are the native sidebar style now; a toggle to revert them isn't worth carrying |
| ~~`ui.outer_pane_borders = false`~~ | `pr/outer-pane-borders` | dropped 2026-08-13: upstream #2535 shipped `ui.pane_outer_borders` as the native toggle, so the fork's overlapping key came off rather than carry two spellings of the same feature. The fork's take was richer (tmux-exact divider coloring, gapless splits, mouse hit-testing); the branch is kept for reference if upstream's simpler version proves insufficient |
| ~~popup `chrome = "modal"`~~ | *(branch deleted 2026-07-27)* | dropped: upstream popups already draw an accent border, an in-border title, and an opaque panel background (`PopupChrome::Pane`), so the patch only added a dimmed backdrop and a second header row. Not worth carrying. The commit survives inside `pr/popup-geometry-defaults`, and the master-side originals in `backup/master-f2facce` |
| ~~`ui.pane_borders = "between"`~~ | `pr/split-only-pane-borders` | superseded 2026-08-01 by `ui.outer_pane_borders` (itself dropped 2026-08-13 once upstream #2535 landed `ui.pane_outer_borders` — see the row above). Came off because the divider read as one flat color; that is actually tmux's own behavior for a two-pane split (the single divider borders both panes, so it highlights either way), and the replacement keeps it deliberately. The real problem was the spelling: overloading `pane_borders` changed a key's type and dragged in the `[hdev]` overlay. Branch kept for reference only |
| `[ui.popup]`: fallback `width`/`height`/`chrome` for popups that declare none — the only way to resize a plugin manifest pane without editing the plugin | `pr/popup-geometry-defaults` | needs more work; carries the deleted modal-chrome commit as its base (it needs `chrome`), so strip that out before this could go upstream. Also stacked on `pr/workspace-id-test-isolation` |
| ~~`HERDR_BIN_PATH` in every pane~~ | *(branch deleted 2026-09-12)* | dropped: upstream's `apply_pane_base_env` now exports `HERDR_BIN_PATH` next to `HERDR_SOCKET_PATH` for every pane, which is exactly what the branch did |
| ~~`ui.dim_unfocused_panes`~~ | *(never branched separately)* | dropped 2026-09-12: upstream stopped dimming unfocused pane content when rendering moved into the client (#3487), so the toggle had nothing left to switch off. `pr/focused-pane-styles` keeps only the two border colors |
| ~~macOS manual builds via Homebrew `zig@0.15`~~ | *(never branched)* | dropped 2026-09-12: upstream builds with Zig 0.16.0 through setup-zig since the libghostty upgrade (#3906); pinning `zig@0.15` would install a compiler the vendored libghostty-vt no longer accepts |
| workspace id length test no longer depends on how many workspaces earlier tests allocated from the global counter | `pr/workspace-id-test-isolation` | test-only; came off with `pr/popup-geometry-defaults`, the only thing that needed it |
| `[hdev]` config overlay: tables under `[hdev]` are deep-merged over the matching top-level sections before the config is deserialized, so one `config.toml` works on both binaries | `fork/hdev-config-overlay` | **fork-only, never upstream**; only earns its keep while a fork patch changes an existing key's *type*, and none do. `ui.outer_pane_borders` was spelled as a new key specifically to avoid needing this |

To open a PR later:

```bash
git push origin <branch>
gh pr create --repo ogulcancelik/herdr --head adkala:<branch>
```

The staged `pr/*` branches for the rows above were re-cut on 2026-09-12 from
the listed `master` commits onto upstream `d184b41`, so each still applies with
a plain cherry-pick; `pr/sidebar-tab-label` stacks on `pr/auto-tab-titles`.
The branches in the table below (`pr/popup-geometry-defaults`,
`pr/workspace-id-test-isolation`, `pr/outer-pane-borders`,
`pr/split-only-pane-borders`, `fork/hdev-config-overlay`) stay on their older
bases.

When adding a new change, commit it on a `pr/<slug>` branch based on
`upstream/master`, cherry-pick it into `master`, and add a row above. Branches
that must never go upstream take the `fork/<slug>` prefix instead, so `pr/*`
stays a safe glob to push.

---

# herdr


<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **detach without stopping work** — herdr keeps terminals running in a background server when you close the client or lose your SSH connection. after a server or machine restart, herdr restores the saved layout and can resume supported agent sessions; the original processes do not survive. [session state →](https://herdr.dev/docs/session-state/)
- **several machines, one window** — keep local work and saved ssh machines together, with a combined agent list and independent reconnects. [remote machines →](https://herdr.dev/docs/connecting-machines/)
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [endpoint-protected Windows](https://herdr.dev/docs/windows-beta/) · [binaries](https://github.com/herdrdev/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [connecting machines](https://herdr.dev/docs/connecting-machines/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/herdrdev/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
