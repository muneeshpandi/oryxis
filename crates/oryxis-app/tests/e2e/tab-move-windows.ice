viewport: 1200x750
mode: Zen
-----
# A tab moves between windows with its session: out into a window of its
# own ("Move to New Window"), and into another open window ("Move to
# Window: <what it shows>"). Every tab stays in the app; a window only
# lists the ones it shows, so the strips are what this asserts.
#
# The emulator draws ONE window and adopts the last one opened, so after
# the move it is showing the new window: its strip holds the moved tab
# alone, and its menu offers the other window instead of a new one. The
# move back leaves that window empty, so it closes, and the emulator's
# id then names no window, which the app draws as the first one.
#
# Dragging a tab past the window's edge is owner QA: the harness has no
# press-move-release.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
settle 250
click (92, 20)
expect "Local Shell"
timeout 800
click "Local Shell"
settle 900
# Name the first tab so the two can be told apart. The rename field
# opens on the current label, so it is selected before typing over it.
click right "bash (default)"
settle 250
click "Rename Tab"
settle 250
type ctrl+a
type "alpha"
type enter
settle 300
expect "alpha"
click (333, 20)
expect "Local Shell"
click "Local Shell"
settle 900
expect "bash (default)"
# Out: the extra window shows the renamed tab and nothing else.
click right "alpha"
settle 250
expect "Duplicate in New Window"
click "Move to New Window"
settle 600
expect "alpha"
absent "bash (default)"
click right "alpha"
settle 250
expect "Move to Window: bash (default)"
absent "Move to New Window"
# Back: the first window's strip holds both again.
click "Move to Window: bash (default)"
settle 600
expect "alpha"
expect "bash (default)"
click right "alpha"
settle 250
expect "Move to New Window"
absent "Move to Window:"
# "Duplicate in New Window": the copy is born in a new extra window,
# which shows it alone (a duplicate takes the shell's own label, not the
# rename).
click "Duplicate in New Window"
settle 1200
expect "bash (default)"
absent "alpha"
