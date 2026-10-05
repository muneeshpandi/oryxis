viewport: 1200x750
mode: Zen
-----
# Ctrl+Shift+D duplicates the focused tab (Windows Terminal's chord; the
# side-by-side split moved to Alt+Shift+Plus). The copy's label is the
# shell's, which differs per machine, so the proof is the close count:
# with the copy open, the first Ctrl+Shift+W still leaves a tab, and only
# the second lands on Home. Without the duplicate the first close lands
# there already.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
expect "Create host"
type ctrl+shift+l
settle 1500
absent "Create host"
type ctrl+shift+d
settle 1500
type ctrl+shift+w
settle 800
absent "Create host"
type ctrl+shift+w
settle 800
expect "Create host"
