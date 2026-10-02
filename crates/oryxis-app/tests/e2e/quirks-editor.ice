viewport: 1000x2560
mode: Zen
-----
# C5: the host editor's "Advanced terminal" section (per-host legacy
# keyboard modes + feature toggles). A tall viewport so the whole form
# fits (the side-panel scrollable can't be wheel-scrolled headless).
# This smoke test proves the section is wired and its i18n resolves; the
# byte-level encoding + the form save/load round-trip are unit-tested
# (util.rs vectors, the vault + core round-trip tests). The pick-list
# option overlays aren't text-selectable, so flipping a value is left to
# the unit tests. The target is a bare name on purpose: an explicit one
# (username / port / IP literal) makes the empty state quick-connect and
# relabel the button to Connect (#97, 170654f0), so the editor never
# opens.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
expect "Create host"
click "Create host"
settle 300
click "Type IP or Hostname"
type "quirkshost.example.com"
click "Continue"
settle 500
# Two-tier editor: the quirks live in the collapsed Compatibility
# section (with the legacy-algorithm pickers); open it first.
expect "New Host"
click "Compatibility"
settle 300
expect "Legacy algorithms"
expect "Advanced terminal"
expect "Changes reach this host's open sessions at once. The rekey limit applies on the next connection."
expect "Backspace key"
expect "Home/End keys"
expect "Function keys"
expect "Report mouse to remote"
expect "Off: the mouse stays here. Clicks select and paste locally, the wheel scrolls the scrollback, and in full-screen apps like vim or less it sends nothing, not even arrow keys. On: hold Shift to keep one gesture local."
expect "Clipboard (OSC 52)"
expect "Rekey limit (MB)"
screenshot quirks-editor
