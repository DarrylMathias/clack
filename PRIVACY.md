# clack Privacy Statement

clack reads keyboard events from your operating system to know which key was
pressed, so it can play the matching sound. It does this by:

- **Linux:** reading from `/dev/input/event*` (opened read-only, never grabbed)
- **macOS:** using a listen-only `CGEventTap`

**clack never logs, stores, or transmits any keystrokes or key codes.**
The only data written to disk is the mute state and your settings
(`~/.config/clack/config.toml`, `~/.local/state/clack/state.toml`).

The udev rule installed on Linux (`/etc/udev/rules.d/70-clack.rules`) grants
the currently logged-in user read access to keyboard event nodes. This is the
same trust level that X11 had by default. Running `clack uninstall` removes it.

If you have questions or concerns, open an issue at
https://github.com/DarrylMathias/clack
