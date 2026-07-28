## fork changes (adkala/herdr)

This fork carries the changes below on `master`. Each is meant to stay
cherry-pickable onto the current `upstream/master`
([ogulcancelik/herdr](https://github.com/ogulcancelik/herdr)), so it can become an
upstream PR without untangling anything. Staged branches are pushed to `origin`
(this fork), so a fresh clone has them.

| change | commit on `master` | staged branch | notes |
| --- | --- | --- | --- |
| `advanced.escape_time_ms`: configurable lone-Escape flush delay, like tmux `escape-time` (default 10ms) | `5c66609` | — | PR candidate; lives on `master` only |
| `advanced.osc52_paste`: opt-in OSC 52 paste support — answers `OSC 52 ; c ; ?` clipboard read queries (off by default). `true`/`"server"` replies with the server machine's clipboard; `"terminal"` forwards the query to the local terminal so panes paste from the local clipboard over ssh | `c8f9a53`, `b7cf3dd`, `59214d5` (e2e test), `a3d6b5c` (docs) | — | PR candidate; lives on `master` only |
| macOS manual artifact builds install patched Homebrew `zig@0.15` instead of setup-zig | `e895ee7` | — | mainly serves this fork's `dev` prerelease builds; upstreamable if wanted |
| manual artifact builds stamp `HERDR_BUILD_CHANNEL=dev` + `HERDR_BUILD_ID=<short sha>`, so binaries report `herdr <version>-dev.<sha>` | `8b1a3da` | — | fork-only build identity; pairs with the row above |
| `ui.focused_pane_border` / `ui.unfocused_pane_border` / `ui.dim_unfocused_panes`: separate focused-pane styling like tmux `pane-active-border-style` / `pane-border-style` / `window-style` shading | `49a466c` | `pr/focused-pane-styles` | PR candidate |

### staged, not on `master`

Reverted from `master` on 2026-07-27: each needs more work before it ships on the
`dev` channel. Every one is intact on the branch below — nothing was discarded.
`master` was rebuilt as upstream `bb29eed` plus the rows above, and the
pre-revert tip is kept at `backup/master-f2facce`.

To reinstate one, cherry-pick its branch onto `master` and move its row up.

| change | branch | why it came off |
| --- | --- | --- |
| `ui.sidebar_worktree_connectors`: set `false` to drop the worktree tree connectors (`├─`/`└─`) and trailing group chevron added by upstream #1873, restoring the flat indent style with a leading chevron | `pr/sidebar-worktree-connectors` | needs more work |
| popup `chrome = "modal"`: popup panes (`plugin.pane.open`, `herdr plugin pane open --chrome`, `type = "popup"` keybinds) render with the built-in settings overlay treatment — dimmed backdrop, panel shell, bold title header row; popup keybinds title the header from `description` | `pr/popup-modal-chrome` | needs more work |
| `ui.pane_borders = "between"`: tmux-style near-borderless splits — only the shared divider between panes is drawn, no outer frame; zoomed/single panes draw nothing | `pr/split-only-pane-borders` | needs more work; it changes an existing key's type, so it also needs the `[hdev]` overlay below |
| `[ui.popup]`: fallback `width`/`height`/`chrome` for popups that declare none — the only way to resize a plugin manifest pane without editing the plugin | `pr/popup-geometry-defaults` | needs more work; stacked on `pr/popup-modal-chrome` (needs `chrome`) and `pr/workspace-id-test-isolation` |
| `HERDR_BIN_PATH` in every pane, not just plugin commands, plugin panes, and custom command keybinds — panes already get the socket, so they can address the server but still have to guess a client, and the API rejects a protocol mismatch outright | `pr/pane-bin-path` | needs more work |
| workspace id length test no longer depends on how many workspaces earlier tests allocated from the global counter | `pr/workspace-id-test-isolation` | test-only; came off with `pr/popup-geometry-defaults`, the only thing that needed it |
| `[hdev]` config overlay: tables under `[hdev]` are deep-merged over the matching top-level sections before the config is deserialized, so one `config.toml` works on both binaries | `fork/hdev-config-overlay` | **fork-only, never upstream**; only earns its keep while a fork patch changes an existing key's *type* (see `ui.pane_borders`), and none currently do |

To open a PR later:

```bash
git push origin <branch>
gh pr create --repo ogulcancelik/herdr --head adkala:<branch>
```

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
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a> · <a href="#sponsors">sponsors</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/ogulcancelik/herdr/releases"><img src="https://img.shields.io/github/downloads/ogulcancelik/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/ogulcancelik/herdr/stargazers"><img src="https://img.shields.io/github/stars/ogulcancelik/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/ogulcancelik/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/ogulcancelik/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**agent multiplexer that lives in your terminal.**

- **every agent at a glance** — blocked, working, done. real terminal views, not a wrapped interpretation.
- **detach, agents keep running** — reattach from any terminal, or over ssh. sessions survive restarts.
- **agents can use herdr too** — a pure socket api: agents spawn panes, read output, wait on each other. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows beta: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [binaries](https://github.com/ogulcancelik/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## sponsors

herdr is built full-time, in the open. sponsoring directly funds development, stability, and the path to a real agent runtime.

### gold

<a href="https://terminaltrove.com/"><img src="assets/sponsors/terminal-trove.png" alt="Terminal Trove" width="200" /></a>

[**→ become a sponsor**](https://github.com/sponsors/ogulcancelik) · enterprise / partnership: hey@herdr.dev · see [SPONSORS.md](./SPONSORS.md) for tiers. thank you 🐑

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/ogulcancelik/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
