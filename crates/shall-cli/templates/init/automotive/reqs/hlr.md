# L1 – High-level requirements

## Protect the motor against overheating
`@@hlr~motor-overheat-protection@@`

The drive protects the motor against damage by overheating.

Rationale:
An example to start from: replace it with your first requirement, and
create further IDs with `shall new hlr "<title>" --file reqs/hlr.md`.

## Report the motor temperature
`@@hlr~report-motor-temperature@@`

The drive reports the motor temperature to the vehicle once a second.

This requirement needs no system requirement of its own, so the line
below forwards it to the software requirements: software requirements
cover `@@hlr~report-motor-temperature|sys@@` instead.

sys --> swr : @@hlr~report-motor-temperature@@
