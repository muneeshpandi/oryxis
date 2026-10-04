viewport: 1200x750
mode: Zen
-----
# Each window has its own place in the vault screens: a search typed in
# one window's host list does not filter another window's. The hosts
# themselves are the app's, so both windows list the same ones.
#
# The emulator shows the last window opened and falls back to the first
# once that one closes (see tab-move-windows.ice). (300, 119) is the
# host search field, (19, 20) the burger, (1176, 20) the close button.
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
# A search that matches nothing hides the host in THIS window.
click (300, 119)
type "zzzz"
settle 300
absent "web01"
# A new window starts with no search: the host is listed.
click (19, 20)
settle 300
click "New Window"
settle 700
expect "web01"
# And the first window kept its own search.
click (1176, 20)
settle 700
absent "web01"
