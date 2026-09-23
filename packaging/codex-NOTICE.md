# Magicodex notice

`bin\codex.exe` in this package is a modified build of OpenAI Codex CLI, built from the
`rust-v0.153.4` source release (<https://github.com/openai/codex/tree/rust-v0.153.4>) with the
patches in `patches\` applied:

| Patch | Change |
| --- | --- |
| `0001-release-lock-alignment.patch` | Aligns the workspace package versions in the lock file and the version snapshots with the 0.153.4 release |
| `0002-native-magic-tui.patch` | Adds the magic circle to the TUI and the `/magic on\|off\|list` command |
| `0003-downward-reply.patch` | The final reply comes out below the circle; streaming preview |
| `0004-visual-refresh.patch` | Redrawn circle: layered hexagram, text bands, light sweep, outlet |
| `0005-magic-styles.patch` | Ten circle styles and the `/magic list` picker |

`patches\manifest.json` lists every changed file with checksums. The modifications are
provided under the Apache License, Version 2.0 (see `LICENSE`), like the original source. The
original notices are in `NOTICE`.

`bin\codex-code-mode-host.exe`, `codex-resources\codex-command-runner.exe`,
`codex-resources\codex-windows-sandbox-setup.exe`, `codex-path\rg.exe` and `codex-package.json`
are unmodified copies from the official npm package `@openai/codex-win32-x64` 0.153.4
(Apache-2.0). `rg.exe` is ripgrep by Andrew Gallant, dual-licensed under the MIT license and
the Unlicense (<https://github.com/BurntSushi/ripgrep>).

The launch scripts (`magicodex.cmd`, `magicodex-bridge.cmd`, `scripts\Start-NativeBridge.mjs`)
are part of Magicodex and licensed under the MIT license:

```text
MIT License

Copyright (c) 2026 Magicodex contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

This is not an official OpenAI release, and it is not affiliated with or endorsed by OpenAI.
