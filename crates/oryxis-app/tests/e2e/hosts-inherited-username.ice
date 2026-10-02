viewport: 1240x1500
mode: Zen
-----
# A host that leaves its username blank inside a folder with a default
# user logs in as that user, and the card says so (issue #242): the
# subtitle used to read the raw field and fall back to root while the
# dial inherited deploy. The search reads the same resolved login, so
# the folder's user finds the host too.
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
# The hostname field opens focused; the username stays blank.
type "web01"
click "My Server"
type "web01"
click "Save"
settle 250
expect "web01"
# Settings > Interface > "Show host address": the toggler itself, at
# the trailing edge of its row (the label is not the control).
click (19, 20)
settle
click "Settings"
settle
expect "Interface"
click (1182, 418)
settle
click (57, 20)
settle
expect "deploy@web01 · Auto"
# The inherited login is searchable, like a typed one.
click "Search hosts or quick connect (user@host)..."
type "deploy"
settle 250
expect "deploy@web01 · Auto"
