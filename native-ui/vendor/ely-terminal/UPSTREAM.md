Upstream: https://github.com/ELY-Open-Source/Ely-GPUI-Components
Commit: 69a62b10168eed5b011b978497c2fdca644cb81f
License: MIT (LICENSE)

DevShell additions in src/view.rs: a remote input/resize callback, incremental
output parser, and per-terminal appearance. The existing selection, IME,
scrollback, search, VT modes, clipboard and grid painter are retained. No local
PTY is started for SSH, serial or TCP sessions.
