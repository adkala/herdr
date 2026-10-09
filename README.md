## fork changes (adkala/herdr)

This fork carries the changes below on `master`. Each is meant to stay
cherry-pickable onto the current `upstream/master`
([ogulcancelik/herdr](https://github.com/ogulcancelik/herdr)), so it can become an
upstream PR without untangling anything. Staged branches are pushed to `origin`
(this fork), so a fresh clone has them.

| change | commit on `master` | staged branch | notes |
| --- | --- | --- | --- |
| `advanced.escape_time_ms`: configurable lone-Escape flush delay, like tmux `escape-time` (default unset — 10ms, or 150ms while mouse capture holds a pending Escape) | `57387e1`, `1116fdb` (thin-client fix), `8a44bdd` (docs) | `pr/escape-time-ms` | PR candidate. The first commit alone was inert: it wired the key only into the in-process reader, while the thin client hardcoded its flush window. Since the 2026-09-12 rebuild only the thin-client reader exists (upstream #3487 removed the in-process `--no-session` path), so `5e218ec` now carries just the config key and `56b4a8d` the behavior. The branch squashes all three into one cherry-pickable commit. Upstream 0.9.1–0.9.3 gave the reader two more waits (50ms for a pending `ESC [` under mouse capture, 500ms for a delayed mouse tail from a host that reports Escape as `CSI 27 u`); the patch leaves both alone and still owns only the lone-Escape window |
| `advanced.osc52_paste`: opt-in OSC 52 paste support — answers `OSC 52 ; c ; ?` clipboard read queries (off by default). `true`/`"server"` replies with the server machine's clipboard; `"terminal"` forwards the query to the local terminal so panes paste from the local clipboard over ssh. A `"server"` machine with no clipboard (headless ssh host) hands the query to its client, and a `"server"` client with a clipboard answers from it, so one value works on both ends of `herdr --remote` | `f0421c0`, `dba73b2`, `9bd9fbd` (e2e test), `8f131e2` (docs), `f96624d` (endpoint controls), `9e854c9` (client clipboard for headless servers) | — | PR candidate; lives on `master` only. The `"terminal"` mode rides the stable endpoint lane as named controls: the client hello advertises the `host_clipboard_query` capability (new optional `capabilities` list), the server asks the foreground shell with `endpoint.clipboard.query.v1`, and the shell relays its terminal's answer as `endpoint.clipboard.reply.v1` (`{"data":"<base64>"}`, capped at 512 KiB). A foreground client that did not advertise the capability (stock shell, direct `pane attach`) gets an immediate empty reply instead of a 5 s stall. No enum variants are appended, so the private protocol number is untouched |
| manual artifact builds stamp `HERDR_BUILD_CHANNEL=dev` + `HERDR_BUILD_ID=<short sha>`, so binaries report `herdr <version>-dev.<sha>` | `c8304a6` | — | fork-only build identity. The workflow otherwise follows upstream, including the Zig 0.16.0 setup-zig step |
| `ui.focused_pane_border` / `ui.unfocused_pane_border`: separate focused-pane border colors like tmux `pane-active-border-style` / `pane-border-style` | `48f1ba6` | `pr/focused-pane-styles` | PR candidate. The original `ui.dim_unfocused_panes` toggle came off in the 2026-09-12 rebuild (see below) |
| `ui.tab_titles = "terminal_title"`: auto-named tabs inherit their focused pane's terminal title, like tmux automatic-rename (default stays numbered tabs) | `97057ab` | `pr/auto-tab-titles` | PR candidate. Since upstream #3487 the client shell draws whatever tab label the server projects, so the inherited title reaches the tab strip, mobile header, navigator and window title through the shell snapshot; a title change forces a snapshot refresh the same way sidebar title tokens do |
| attached clients render colored underlines: the in-memory `CellData` carries the SGR 58 underline color and the client emits `58:2::r:g:b` / `58:5:n`, so Neovim's red diagnostic undercurls stay red through herdr (they fell back to the text color). The color stays out of every published cell layout and travels in an optional `endpoint.surface-underline.v1` control (`[x, y, len, color]` runs for the frame and popup) that the server writes just ahead of the surface update it belongs to, in the same client write | `64a2b7d`, `0223750` (endpoint control) | `pr/underline-color` | PR candidate; upstream bug (#1252, #1169, #1178 were still open when last checked on 2026-09-12), fork carries it until merged. Redesigned in the 2026-10-05 rebuild: upstream 0.9.1–0.9.2 added three more surface encodings (the reuse, delta and scroll controls) that each embed the six-field cell, so the negotiated `shell.surface.v2` codec this fork spoke on 0.9.0 would have needed a second copy of every one. Now the cell's wire layout never changes (serde skips the field and the frozen generation-1 digests are upstream's), the client hello asks with `surface_underline_color`, the server advertises the `surface_underline_color` capability, and a control is sent only for an update that contains a colored underline. The client's surface decoder pairs the runs with their update by boot id and surface revision and paints its own baseline too, so cells a later reuse, delta or row shift carries over keep their color. A fork client on an upstream server, or a stock client on a fork server, gets no control and underlines in the text color; so does a fork build from before this rebuild (those offered `shell.surface.v2` and fall back to v1), so upgrade client and server together to keep the colors. The private protocol stays at 22. The branch is one squashed commit that threads the hello flag like upstream's own `surface_*` flags rather than through the OSC 52 patch's negotiation struct |
| custom popup keybinds take their border title from `description` instead of always rendering the literal `popup` | `0c5d9bb` | `pr/popup-title-from-description` | PR candidate; upstream has no other way to name a popup — `popup_pane` sits outside any workspace so `pane.rename` cannot reach it, and `$HERDR_PANE_ID` is unset inside one. Lives in `src/app/custom_commands.rs` since the 2026-09-12 rebuild |
| mobile layout marks the zoomed tab: the header's `tab N · i/n` label and the switcher's per-space detail row carry the desktop tab bar's ` Z` suffix, since mobile draws no tab strip or `tab_bar_right` status area | `d164001` | `pr/mobile-zoom-indicator` | PR candidate; lives in `src/client/shell/mobile.rs` since the 2026-09-12 rebuild |
| sidebar agent rows: the `tab` token shows the title an auto-named tab inherits under `ui.tab_titles = "terminal_title"` (stock only knows custom names and numbers there), and an inherited title fills it even in a single-tab workspace, so `rows = [["state_icon", "workspace"], ["agent", "tab"]]` reads `claude · <title>` | `d982678` | `pr/sidebar-tab-label` | PR candidate; folds into `pr/auto-tab-titles` if that goes upstream. The client shell decides token visibility, so each snapshot tab now carries a serde-defaulted `inherited_label` flag (older snapshots still decode) |
| Kitty graphics for clients started inside tmux: when `TMUX` is set and `terminal.kitty_graphics` is on, the client shell shows pane images as Kitty Unicode-placeholder virtual placements (`U=1` plus `U+10EEEE` cells whose foreground carries the image id and whose underline color carries the placement id) and wraps every Kitty command in `DCS tmux;` passthrough, since tmux never syncs the outer cursor for passthrough | `1ffee87` | `pr/kitty-tmux-placeholders` | PR candidate; stacks on `pr/underline-color`, whose SGR 58 underline color carries the placement id. Needs tmux 3.3+ with `allow-passthrough on` (or `all`), `focus-events on` (with `on`, tmux drops passthrough written while the pane is hidden, so regaining focus resets the host cache and re-uploads everything; `all` avoids the re-upload), and an outer terminal with Unicode placeholder support such as kitty or Ghostty. Placeholder cells replace the text under an image, so `z < 0` images draw over text inside tmux. Clients outside tmux write byte-identical output (a transcript test pins it) and nothing on the wire changes. Redesigned 2026-09-14 for the client-shell graphics path (placeholder painting in the shell's composition, passthrough wrapping in the client's frame writer) and re-fitted in the 2026-10-05 rebuild onto upstream's native Kitty rendering (#4561, which removed the pane graphics API and its plugin layers): each piece cropped around an overlay is a virtual placement of its own on the cells the overlay leaves visible, uploads stay inline behind tmux because it relays no reply to the temporary-file probe, and the e2e test has the pane draw a real Kitty image now that `pane.graphics.set` is gone. Addison's original WIP on upstream `c2637dc1` is kept at `backup/kitty-tmux-placeholders-a4d5acd` |
| Claude integration install accepts a home-relative session hook: a `SessionStart` command spelled `bash "$HOME/.claude/hooks/herdr-agent-state.sh" session` (also `${HOME}`, unquoted, or `~/`) that resolves to the hook path counts as the install, so `herdr integration install claude` no longer appends its absolute-path entry beside it. A canonical entry already sitting next to one is dropped, and uninstall removes the home-relative hook too | `8827625` | `pr/claude-portable-hook` | PR candidate. For a `settings.json` shared between machines (a dotfiles checkout symlinked into `~/.claude`), which cannot hold the absolute hook path: stock appended the canonical entry on every integration update, so the hook ran twice per session and a machine-specific path landed in the shared file. Rebased in the 2026-10-05 rebuild onto upstream's native-source matcher `^(startup\|resume\|clear\|compact\|fork)$` (#4046) and its `write_config` (#3973): a home-relative hook still counts as the install and keeps its own matcher, timeout and position, so narrow the matcher by hand in a shared file to get upstream's fix for Grok's imported hooks. A wildcard entry left beside it by an older install is dropped like the canonical one. Windows is unchanged |
| `distribution/install.sh` installs the fork's Linux dev build from the `adkala/herdr` `dev` release instead of the upstream `herdr.dev` manifest (macOS still uses the manifest); checksum is skipped on that path because the `dev` release ships no sha256 manifest | `203d6e4` | — | **fork-only, never upstream**; points the installer at the fork's own binary so `curl … install.sh \| sh` on Linux gets the dev channel, not upstream stable. Upstream moved the script from `website/` to `distribution/` when the website left this repo |

### staged, not on `master`

Reverted from `master` on 2026-07-27: each needs more work before it ships on the
`dev` channel. Every one is intact on the branch below — nothing was discarded.
`master` was most recently rebuilt on upstream `v0.9.3` (`7b116c0`) on
2026-10-05; the tip before that rebuild is kept at `backup/master-4e5c0a7`.
Earlier rebuilds are at `backup/master-9931b02` (onto `d184b41`, 2026-09-12),
`backup/master-f65c269` (onto `1c76079`, 2026-08-21), `backup/master-54fe477`
(onto `d76657f`, 2026-08-14) and `backup/master-0aed437` (onto `952729e`,
2026-08-13), and the 2026-07-27 pre-revert tip is at `backup/master-f2facce`.
The 2026-10-05 rebuild crossed upstream 0.9.1 to 0.9.3: three optional surface
encodings (reuse, delta and scroll controls), native Kitty rendering in place
of the pane graphics API (#4561), the `ghostty-vt` workspace crate (#4661), a
reworked stdin reader, and the Claude hook matcher (#4046). Nothing upstream
made a fork patch redundant. The underline color moved off its own surface
codec onto an optional endpoint control, the tmux graphics transport was
re-fitted onto the new encoder, and the escape-time, OSC 52, tab-title,
sidebar-token and Claude-hook patches were re-resolved (see their rows).
The base is the release tag, which sits on upstream's release branch five
commits off `upstream/master`; the next rebuild onto `upstream/master` is
still `git rebase --onto upstream/master v0.9.3 master`.
The 2026-09-12 rebuild crossed upstream's client-shell refactor (#3487: the TUI
now renders in each client and `--no-session` is gone), the 0.9.0 release and
the Zig 0.16.0 / libghostty upgrade (#3906), so the tab-title, mobile-zoom,
sidebar-token and popup-title patches were ported into `src/client/shell/` and
`src/app/custom_commands.rs` instead of carried unchanged, the OSC 52 terminal
mode moved onto the client-shell lane (since 2026-09-13 as named endpoint
controls, see its row), and three things came off: the
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
| workspace id length test no longer depends on how many workspaces earlier tests allocated from the global counter | `pr/workspace-id-test-isolation` | test-only; came off with `pr/popup-geometry-defaults`, the only thing that needed it. Re-cut onto upstream `v0.9.3` on 2026-10-05 (`5d07d1c`); upstream still has the `first.len() <= 3` assertion it replaces |
| ~~negotiated `shell.surface.v2` surface codec for underline colors~~ | *(folded into `pr/underline-color`)* | replaced 2026-10-05: it carried a widened cell in appended `PaneSurfaceV2` / `PaneSurfacePatchV2` variants with frozen v1 mirrors for older clients. Upstream's reuse, delta and scroll controls embed the same cell, so each would have needed a v2 twin; the color now rides the optional `endpoint.surface-underline.v1` control instead (see its row). The old implementation is in `backup/master-4e5c0a7` (`a7f1918`) |
| `[hdev]` config overlay: tables under `[hdev]` are deep-merged over the matching top-level sections before the config is deserialized, so one `config.toml` works on both binaries | `fork/hdev-config-overlay` | **fork-only, never upstream**; only earns its keep while a fork patch changes an existing key's *type*, and none do. `ui.outer_pane_borders` was spelled as a new key specifically to avoid needing this |

To open a PR later:

```bash
git push origin <branch>
gh pr create --repo ogulcancelik/herdr --head adkala:<branch>
```

The staged `pr/*` branches for the rows above were re-cut on 2026-10-05 from
the listed `master` commits onto upstream `v0.9.3`, one commit each, so each
still applies with a plain cherry-pick: `pr/escape-time-ms` squashes its three
commits, `pr/underline-color` its two, `pr/sidebar-tab-label` stacks on
`pr/auto-tab-titles`, and `pr/kitty-tmux-placeholders` stacks on
`pr/underline-color`, whose SGR 58 underline color carries the placement id.
`pr/workspace-id-test-isolation` was re-cut onto `v0.9.3` the same day. The
remaining branches in the table above (`pr/popup-geometry-defaults`,
`pr/outer-pane-borders`, `pr/split-only-pane-borders`,
`fork/hdev-config-overlay`) stay on their older bases. Addison's original tmux
graphics WIP on upstream `c2637dc1` is preserved at
`backup/kitty-tmux-placeholders-a4d5acd`. Each branch keeps its
`docs/next/CHANGELOG.md` entry; upstream asks pull requests to leave that file
alone, so drop the hunk when opening one.

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
