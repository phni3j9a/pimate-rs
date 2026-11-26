---
name: pimate-getting-started
description: Set up a computer for PiMate (install Pi and PiMate if missing), check it, and offer phone answers.
---
Follow pimate-rules. Use ListMachines and select the computer the user names; ask if unclear. If it isn't connected, ask them to install, run and sign into the Grok Bot desktop app there. PiMate runs on Linux and macOS (x64 or arm64) with Node.js 22.19+ on 22.x, 23.11+ on 23.x, or 24+.

## Check (read-only)
1. Run `uname -sm`, `node --version` and `pi --version`. Find `pimate` on PATH, else `~/.pi/agent/npm/node_modules/.bin/pimate`.
2. Run `pimate --help`. Its usage must include `wait ID`, `shutdown ID` and `webhook`; otherwise stop here and identify the installed edition (see "Updating"). A missing command alone does not mean PiMate is not installed.
3. Run `pimate list`, `pimate service status` and `pimate webhook status`.

Report what is missing. Don't install, start or change anything without permission.

## Install what is missing
- Unsupported OS, CPU or Node version: tell the user PiMate can't run there yet, and stop.
- No Pi: give the user the official install command from pi.dev, `curl -fsSL https://pi.dev/install.sh | sh`. They then run `pi` once in a terminal there and log in to their model provider themselves. Never handle provider logins or keys.
- No PiMate: after the user's yes, run `pi install npm:@phni3j9a/pimate`. Install only this package, only from npm.
- Skills: read every `SKILL.md` under the installed package's `grok-bot/skills/` (usually `~/.pi/agent/npm/node_modules/@phni3j9a/pimate/grok-bot/skills/`) and, after the user's yes, save each verbatim as a private skill with the same name and description.
- Host: starting Pi and sending to it work without it; the PiMate phone app and phone answers need it. The user opens `pi` in a terminal on that computer and runs its `/pimate setup` command, which can register a user service on Linux with systemd, but only with their consent. On macOS this setup starts a temporary Host, not a login service. Only if the user wants it to start at login, ask them to run `pimate service install` from their logged-in Mac account to register the LaunchAgent.

## Finish
Save the computer's ListMachines label and working CLI command to memory. Explain the desktop app's **Execution on this computer** setting (Settings > Computer): **Ask every time** is recommended for default mode. Unattended delegated mode needs **Always allow**, which lets every Bot run any command on this computer without asking. After a reboot local execution stops until the desktop app runs again.

Offer optional phone answers (pimate-phone-answers "Setup").

## Updating
First check the installed package version (in its `package.json`) and which CLI is in use. The old TS releases are 0.1.x; Rust releases start at 0.2.0. If the installation is missing, mixed or unclear, do not update until the user verifies it.

- **TS → Rust:** Before updating, walk the user through the Rust README's [TS migration steps](https://github.com/phni3j9a/pimate-rs/blob/main/README.md#npm-配布と-ts-版からの切り替えm8-で公開を承認した後). The Rust CLI cannot attach to old abduco terminals. Wait for current work to finish or let the user save it and exit old Pi sessions. Stop a manually started Host in its original terminal. While the old extension is still loaded, have the user record the CLI link and Host version from `/pimate status`, then, with their approval, use `/pimate uninstall`. Verify the old Host stopped and the old service unit (on Linux) and CLI link are gone, including broken symlinks. If cleanup fails, stop; do not replace the package, delete settings or remove pairing data. Only after these checks and the user's yes, update with `pi update npm:@phni3j9a/pimate`, restart Pi and follow the Host setup above. Check `/pimate status`.
- **Rust → Rust:** After the user's yes, run `pi update npm:@phni3j9a/pimate`. If the Host was running, ask the user to run `/pimate restart` in Pi on that computer. Pi sessions keep running while clients reconnect. Running Pi sessions pick up the new version after `/reload` or a restart in Pi.

After either update, with the user's yes, re-read and re-save every pimate-* skill verbatim as above.
