viewport: 1200x750
mode: Zen
-----
# Every window is a whole window: a new one opens on the hosts screen,
# opens tabs of its own, and goes back to its own hosts screen with the
# Home button. Closing it closes its tabs and leaves the other window
# as it was.
#
# The emulator draws ONE window and adopts the last one opened (see
# tab-move-windows.ice), so after "New Window" it is showing the new
# one, and once that closes it shows the first one again. (56, 20) is
# the Home button, (19, 20) the burger, (92, 20) the `+` of an empty
# strip, (1176, 20) the window's close button.
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
# A tab in the FIRST window, so the two strips can be told apart.
click (92, 20)
expect "Local Shell"
timeout 800
click "Local Shell"
settle 900
click right "bash (default)"
settle 250
click "Rename Tab"
settle 250
type ctrl+a
type "first"
type enter
settle 300
expect "first"
# The new window: the hosts screen, the saved host, none of the other
# window's tabs.
click (19, 20)
settle 300
click "New Window"
settle 700
expect "web01"
absent "first"
# A tab of its own, then Home in THIS window.
click (92, 20)
expect "Local Shell"
click "Local Shell"
settle 900
expect "bash (default)"
absent "web01"
click (56, 20)
settle 400
expect "web01"
expect "bash (default)"
absent "first"
# Closing it takes its tab with it; the first window still has its own.
click (1176, 20)
settle 700
expect "first"
absent "bash (default)"
