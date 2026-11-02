# Design decisions

## Stop handler
`@@dsn~stop-handler@@`

An interrupt handler for the stop button switches off the drive.

Code implements this item with the tag `[impl->@@dsn~stop-handler@@]` in
a comment, and a test verifies it with `[utest->@@dsn~stop-handler@@]`;
until then, `shall check` reports it as not covered.

Covers:
- @@req~stop-on-request@@
