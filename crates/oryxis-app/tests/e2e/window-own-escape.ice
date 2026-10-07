viewport: 1200x750
mode: Zen
-----
# A modal is answered only from the window that shows it. The command
# palette is opened in the second window; an Esc typed into the first
# window belongs to that window and leaves the palette standing. The
# same Esc in the second window closes it.
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
type ctrl+shift+p
settle 300
expect "Command palette"
window 1
settle 300
type escape
settle 300
window 2
settle 300
expect "Command palette"
type escape
settle 300
absent "Command palette"
