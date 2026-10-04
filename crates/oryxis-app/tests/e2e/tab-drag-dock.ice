viewport: 1200x750
mode: Zen
-----
# A tab dragged out of its window is carried by the mouse, and released
# over ANOTHER window's tab strip it docks there, session and all.
#
# Two windows: the first holds "alpha" and a second shell, the other is
# a new window moved to (300, 500) on the emulated screen and given a
# tab of its own ("other"). Back in the first window, "alpha" is pressed
# and dragged above the top edge: its own window opens under the cursor
# and takes the focus. The button is still down in the FIRST window, so
# the moves and the release that follow are delivered there, in its
# coordinates, which are the screen's (it sits at the origin). The
# release at (700, 515) is inside the second window's strip, 15 px
# below its top edge: "alpha" lands in that strip, the carried window
# closes, and the second window comes forward showing both its tabs.
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
# The second window, placed and given a tab.
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
# Drag "alpha" out of the first window.
window 1
expect "alpha"
press (140, 20)
move (160, 26)
move (200, 10)
move (200, -60)
settle 500
expect "alpha"
absent "other"
# Carry it over the second window's strip and let go.
move (500, 300)
move (700, 515)
settle 300
release (700, 515)
settle 700
expect "other"
expect "alpha"
# The first window kept its other tab and lost "alpha".
window 1
settle 300
expect "bash (default)"
absent "alpha"
