viewport: 1240x1500
mode: Zen
-----
# Two things sit above the host editor's password field and both leave
# with its first keystroke: the identity suggestions (they show while
# the username was the last field typed into) and the "Inherits <identity>
# from <group>" line (a typed password answers the credential family).
# Either one leaving used to shift the password row and drop its focus
# (the issue #241 sweep). A backspace that brings the line back is the
# proof the focus stayed: it only shows while the password is empty.
click "Skip"
click "Continue without password"
settle 250
# An identity, so the username field has something to suggest.
click "Keychain"
settle 250
click "New Identity"
settle 250
click (1000.00, 136.00)
type "ops"
click (1000.00, 214.00)
type "deploy"
click "Save Identity"
settle 250
click "Hosts"
settle 250
# A folder whose hosts inherit that identity (the identity picker's
# second option).
click "New group"
settle 250
click (940.00, 145.00)
type "Prod"
click "Defaults for hosts in this group"
settle 250
click (955.00, 541.00)
settle 250
click (900.00, 622.00)
settle 250
click "Save"
settle 250
click "Prod"
settle 250
click "HOST"
settle 250
expect "Inherits ops from Prod"
# Typing in the username raises the suggestions above the password.
click (1040.00, 708.00)
type "o"
settle 250
expect "ops"
# First password keystroke: suggestions and the inherited line go.
click (1040.00, 825.00)
type "a"
settle 250
absent "Inherits ops from Prod"
absent "ops"
# Still focused: the backspace empties the field and the line returns.
type backspace
settle 250
expect "Inherits ops from Prod"
