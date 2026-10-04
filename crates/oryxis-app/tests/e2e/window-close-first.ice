viewport: 1200x750
mode: Zen
-----
# No window is the main one: the window the app opened with can be
# closed while another is open, and the app carries on in the other,
# with its tabs untouched.
#
# The emulator draws the focused window. A window the app opens takes
# the focus; `window 1` gives it back to the first one, which is then
# closed with its own close button. The focus falls to the window that
# is left, promoted into the first one's place.
#
# (19, 20) is the burger, (92, 20) the `+` of an empty strip, (1176, 20)
# the window's close button.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
settle 250
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
# The second window, with a tab of its own.
click (19, 20)
settle 300
click "New Window"
settle 700
absent "first"
click (92, 20)
expect "Local Shell"
click "Local Shell"
settle 900
click right "bash (default)"
settle 250
click "Rename Tab"
settle 250
type ctrl+a
type "second"
type enter
settle 300
expect "second"
# Back to the first window, which still shows only its own tab.
window 1
expect "first"
absent "second"
# Close the first window. The second one is what is left.
click (1176, 20)
settle 700
expect "second"
absent "first"
