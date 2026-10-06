viewport: 1200x750
mode: Zen
-----
# The only tab of a window that is one of several IS that window, so
# dragging it drags the window, from the first move. Over another
# window's tab strip the window gives way to its tab: that strip opens a
# gap at the slot under the cursor and draws the tab there, and the
# release docks it in that slot. The emptied window closes.
#
# Window 1 holds "alpha" and a second shell; window 2, placed at
# (300, 500), holds "other" alone. "other" is pressed and moved past the
# drag threshold, which carries window 2 by its grip: the next move,
# (140, -200) in its coordinates, puts it at (300, 280) on the screen.
# The move after that, (-100, -260), is the screen's (200, 20): on
# window 1's strip, between "alpha" and "bash (default)". The button
# stays with window 2 while window 1 is looked at, so the release is
# given in window 2's coordinates again.
#
# (19, 20) is the burger, (92, 20) the `+` of an empty strip.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
expect "Create host"
timeout 800
type ctrl+shift+l
settle 500
click right "bash (default)"
settle 250
click "Rename Tab"
settle 250
type ctrl+a
type "alpha"
type enter
settle 300
type ctrl+shift+l
settle 500
click (19, 20)
settle 300
click "New Window"
settle 700
window 2 at (300, 500)
click (92, 20)
expect "Local Shell"
click "Local Shell"
settle 900
click right "bash (default)"
settle 250
click "Rename Tab"
settle 250
type ctrl+a
type "other"
type enter
settle 300
expect "other"
absent "alpha"
# Drag the window by its only tab, then over window 1's strip.
press (140, 20)
move (150, 24)
settle 300
move (140, -200)
settle 300
move (-100, -260)
settle 300
# Window 1 already shows the tab in its strip (the chips narrow to
# the drag width, so the long label is cut there).
window 1
settle 300
expect "alpha"
expect "other"
release (-100, -260)
settle 700
# Docked: window 1 shows all three, on "other", and is the only window.
expect "alpha"
expect "bash (default)"
expect "● other"
