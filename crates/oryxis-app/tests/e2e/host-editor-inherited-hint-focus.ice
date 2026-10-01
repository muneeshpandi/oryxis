viewport: 1240x1500
mode: Zen
-----
# The "Inherits <value> from <group>" line under an empty username
# leaves with the first keystroke and comes back when the field is
# emptied again (issue #241). The input must keep its focus across
# both: the hint changing used to change the field's widget shape,
# which dropped the input's state and every keystroke after the first.
# A backspace that brings the hint back is the proof the focus stayed.
click "Skip"
click "Continue without password"
settle 250
# A folder whose hosts inherit a username.
click "New group"
settle 250
click (940.00, 145.00)
type "Prod"
click "Defaults for hosts in this group"
settle 250
click (940.00, 465.00)
type "deploy"
settle 250
click "Save"
settle 250
click "Prod"
settle 250
click "HOST"
settle 250
expect "Inherits deploy from Prod"
# Username: the first keystroke takes the hint away.
click (1040.00, 708.00)
type "a"
settle 250
absent "Inherits deploy from Prod"
# Still focused: the second keystroke lands, then emptying the field
# brings the hint back.
type "b"
type backspace
type backspace
settle 250
expect "Inherits deploy from Prod"
# And the field is still focused once the hint is back.
type "c"
settle 250
absent "Inherits deploy from Prod"
