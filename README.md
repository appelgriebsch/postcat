# ⚡ postcat

A modern, keyboard-first Postman alternative that lives in your terminal. Built with [ratatui](https://ratatui.rs).

<img width="2430" height="1678" alt="CleanShot 2026-08-05 at 03 47 43@2x" src="https://github.com/user-attachments/assets/2fa891d7-2fe6-493c-99bf-ed0ea8de47ee" />


## Features

- **Full request builder** — method, URL, query params, headers, four body types (JSON / text / form / none), Bearer & Basic auth
- **Pretty responses** — syntax-highlighted JSON (original key order), status/time/size at a glance, header inspector, scroll + wrap
- **Live streaming** — SSE and chunked responses render as they arrive with tail-follow (scroll up to pause it, `G` to re-arm), a live byte counter, and `esc` to stop the stream
- **Library** — save requests under a name, automatic history of every send, open anything back with a single click or ⏎
- **Environments** — define vars once, use `{{name}}` in URLs, params, headers, bodies, and tokens
- **Keyboard-first** — modal (normal/insert) editing, vim-style movement, context-sensitive hints always visible in the status bar, `?` for the full map
- **Select & copy** — drag across the response or any input field to select it; letting go puts it on the system clipboard (OSC 52, so it works over ssh), and `y` copies the whole body when nothing is selected
- **Mouse where it helps** — click panes, tabs, and rows; click into the url or the body to edit right where you clicked; click the method chip for a dropdown picker; scroll the response; the pointer is a normal arrow everywhere and becomes a text I-beam only over the field you're actively editing (terminals with OSC 22)
- **Workspace persistence** — draft, saved requests, history, and env vars survive restarts (`~/Library/Application Support/postcat` or `~/.config/postcat`)

## Installation

### From crates.io

Install the latest release with Cargo:

```bash
cargo install postcat
```

This requires Rust 1.85 or newer. To update an existing installation:

```bash
cargo install postcat --force
```

### From source

```bash
git clone https://github.com/egoist/postcat.git
cd postcat
cargo install --path . --locked
```

## Run

If you installed the binary with Cargo, start postcat with:

```bash
postcat
```

When working from a checkout, you can also run it directly:

```bash
cargo run --release
```

## Keys

| | |
|---|---|
| `⇥` / `1‑4` | move between library · url · request · response |
| `i` / `u` | edit the URL (`⏎` sends, `esc` keeps, `^u` clears) |
| `m` / `M` | cycle method · click the `GET ▾` chip for a menu |
| `s` or `⏎` | send · `esc` cancels in flight |
| `[` `]` / `h` `l` | switch tabs |
| `⏎` `a` `d` `␣` | edit · add · delete · toggle rows |
| `t` / `f` | body & auth type · format JSON |
| `^s` `n` `e` | save request · new request · environment |
| `j` `k` `g` `G` `^d` `^u` `w` | scroll & wrap the response |
| drag · `⇧`+`←`/`→` | select text · `y` copies it (or the whole body) |
| `^b` | toggle the library sidebar |
| `?` | help · `q` quit |

Editing is spreadsheet-style: opening a filled cell selects its text, so typing replaces and arrows keep it.

Selections are copied when you let go of the button, so the flow is the one the terminal took away when the app started capturing the mouse: drag, and it's on the clipboard. Password fields are the exception — they select, but never copy.

## Tests

```bash
cargo test
```

The suite is end-to-end and needs no fixtures, mocks, or network access. Each test
drives the real app the way you do — synthesised key and mouse events go through
the actual dispatch in `keys`, the actual state machine in `app`, and the actual
renderer in `ui` — and then asserts against the rendered terminal buffer via
ratatui's `TestBackend`. Requests go over a real socket to a throwaway HTTP server
(`tests/common/server.rs`) bound to an ephemeral port, which records what was sent
so tests can check the wire, and can dribble out `text/event-stream` events to
exercise streaming.

Covered: sending every method, query params, headers, JSON/text/form bodies,
Bearer and Basic auth, `{{var}}` substitution, response rendering and scrolling,
SSE streaming (incremental render, tail-follow, cancellation), the request
library, the method dropdown, mouse hit-testing, and workspace persistence.

Tests run against an **ephemeral** app (`App::ephemeral`) with no workspace file,
so they never read or write your real saved requests.

Layout:

```
tests/
  e2e.rs              the tests
  common/harness.rs   drives input, renders frames, finds text on screen
  common/server.rs    real HTTP server: echo, status codes, SSE, slow responses
```
