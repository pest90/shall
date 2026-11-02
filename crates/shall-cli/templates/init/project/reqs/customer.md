# Customer requirements

## Shut down the drive on overtemperature
`@@hlr~overtemp-shutdown@@`

The drive shuts down within 100 ms when the winding temperature exceeds
85 °C.

Rationale:
An example to start from: replace it with your first customer
requirement, and create further IDs with
`shall new hlr "<title>" --file reqs/customer.md`.

Comment:
Name the platform capabilities that meet this requirement in a
`Satisfied-by:` list, with their parameter values in `Params:`, for
example:

```markdown
Satisfied-by:
- cap~overtemp-protection-xxxx~1

Params:
- motor.shutdown_time_ms = 100
```

Until then, it needs a software requirement of this project
(`Needs:` of the level).
