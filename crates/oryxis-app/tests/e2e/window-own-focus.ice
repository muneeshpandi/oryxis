viewport: 1200x750
mode: Zen
-----
# Focus belongs to the window it was asked in. Both windows show the
# hosts screen, so their search fields share one widget id; Ctrl+F in
# the second window focuses its search and only its own. Typing in the
# first window afterwards reaches no field there, so its host list is
# not filtered.
#
# (19, 20) is the burger.
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
click (19, 20)
settle 300
click "New Window"
settle 700
expect "web01"
type ctrl+f
settle 300
window 1
settle 300
type "zzzz"
settle 300
expect "web01"
