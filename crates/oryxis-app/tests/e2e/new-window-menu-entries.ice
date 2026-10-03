viewport: 1200x750
mode: Zen
-----
# The menu doors into another window: "Connect in New Window" on a host
# card, and "Duplicate in New Window" / "Move to New Window" on a tab
# whose session is a local shell.
#
# Only the entries are asserted here; what the windows show after a move
# is tab-move-windows.ice. Dragging a tab or a card past the window's
# edge is owner QA: the harness has no press-move-release.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
settle 250
click "Type IP or Hostname"
type "web01"
click "Continue"
settle 250
click "My Server"
type "web01"
click "Save"
settle 250
expect "web01"
click right "web01"
settle 250
expect "Connect in New Window"
type escape
settle 250
absent "Connect in New Window"
# A local shell from the `+` popover, then its chip's menu.
click (92, 20)
expect "Local Shell"
timeout 800
click "Local Shell"
settle 900
click right "bash (default)"
settle 250
expect "Duplicate in New Window"
expect "Move to New Window"
