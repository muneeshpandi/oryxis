viewport: 1200x750
mode: Zen
-----
# An SFTP tab moves between windows like a terminal tab does, and its
# browsing goes with it. The live SFTP buffer is one, and it follows the
# window in use: the tab travels parked in its own slot, the window it
# lands in draws it from there, and a click in that window acts on it.
#
# The emulator draws ONE window and adopts the last one opened (see
# tab-move-windows.ice), so after the move it shows the extra window.
# "shots" is the harness's own screenshot folder inside the sandbox
# home, the one entry the local pane is sure to list: finding the
# listing in the extra window, and again after the move back, is what
# shows the same state being drawn from wherever it sits.
click "Skip"
click "Continue without password"
settle
click (92, 20)
expect "Local Shell"
timeout 800
click "Local Shell"
settle 900
type ctrl+shift+e
settle 500
# The new tab opens on its host picker; a click beside it dismisses it.
click (785, 166)
settle 500
expect "Pick a host to start."
expect "shots"
# Out: the extra window shows the SFTP surface and no terminal tab.
click right "SFTP"
settle 300
click "Move to New Window"
settle 700
expect "Pick a host to start."
expect "shots"
absent "bash (default)"
click right "SFTP"
settle 300
expect "Move to Main Window"
absent "Move to New Window"
# Back: the main window comes up on the SFTP surface, with both chips.
click "Move to Main Window"
settle 700
expect "Pick a host to start."
expect "shots"
expect "bash (default)"
