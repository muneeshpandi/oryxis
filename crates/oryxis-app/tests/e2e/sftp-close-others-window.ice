viewport: 1200x750
mode: Zen
-----
# "Close Other Tabs" on an SFTP tab closes the other SFTP tabs of the
# window it is used in, not every SFTP tab in the app: a tab another
# window shows is not on screen here to be closed from here.
#
# Two SFTP tabs in the first window, "one" (renamed) and "SFTP". "one"
# moves to an extra window, which then gets a second SFTP tab of its
# own. Closing the others THERE leaves "one" alone in that window, and
# the first window's "SFTP" tab is still in its strip when "one" comes
# back. The emulator shows the last window opened, so the first strip is
# only visible again after the move back (see tab-move-windows.ice).
click "Skip"
click "Continue without password"
settle
type ctrl+shift+e
settle 500
# Each new SFTP tab opens on its host picker; a click beside it
# dismisses it.
click (785, 166)
settle 500
click right "SFTP"
settle 300
click "Rename Tab"
settle 300
type ctrl+a
type "one"
type enter
settle 300
expect "one"
click right "one"
settle 300
click "New Tab"
settle 500
click (785, 166)
settle 500
expect "SFTP"
# Alone in its new window, "one" has no other tab to close.
click right "one"
settle 300
click "Move to New Window"
settle 700
expect "one"
absent "SFTP"
click right "one"
settle 300
absent "Close Other Tabs"
# A second SFTP tab of this window's own, then close the others here.
click "New Tab"
settle 500
click (785, 166)
settle 500
expect "SFTP"
click right "one"
settle 300
click "Close Other Tabs"
settle 500
expect "one"
absent "SFTP"
# Back in the first window, its own SFTP tab was never touched.
click right "one"
settle 300
click "Move to Window: SFTP"
settle 700
expect "one"
expect "SFTP"
