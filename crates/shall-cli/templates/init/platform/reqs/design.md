# Design decisions

## Temperature monitor
`@@dsn~temperature-monitor@@`

A periodic task compares the winding temperature with the limit and
switches off the motor.

Code implements this item with the tag `[impl->@@dsn~temperature-monitor@@]`
in a comment, and a test verifies it with
`[utest->@@dsn~temperature-monitor@@]`; until then, `shall check` reports
it as not covered.

Covers:
- @@swr~overtemp-shutdown@@
