viewport: 1200x750
mode: Zen
-----
# No window is the main one: the window the app opened with can be
# closed while another is open, and the app carries on in the other,
# with its tabs untouched.
#
# The emulator draws ONE window, adopts the last one opened, and once
# that one closes it draws under an id that names no window, which the
# app treats as the first (resident) one. That is how this reaches the
# first window while a second is still open: a third window is opened
# and closed, which hands the emulator back to the first. Closing THAT
# promotes the second window into its place, and the emulator, still on
# an id that names no window, now shows the second window's strip.
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
# A third, only to be closed: the emulator comes back to the first.
click right "second"
settle 250
click "Duplicate in New Window"
settle 1200
absent "second"
click (1176, 20)
settle 700
expect "first"
absent "second"
# Close the first window. The second one is what is left.
click (1176, 20)
settle 700
expect "second"
absent "first"
