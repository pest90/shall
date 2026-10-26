# L3 – Software Requirements

## Shut down the motor on overtemperature
`swr~motor.overtemp-shutdown-k4f2~2`

The software shuts down the motor within 100 ms when the winding
temperature exceeds 85 °C.

Rationale:
Revision 2 lowered the reaction time from 200 ms to 100 ms.

Status: approved

Covers:
- sys~motor.thermal-protection-q7m2~1

Needs: dsn

## Warn before overtemperature
`swr~motor.overtemp-warning-p8n5~1`

The software reports a warning when the winding temperature exceeds
75 °C.

Status: proposed

Covers:
- sys~motor.thermal-protection-q7m2~1

Needs: dsn

## Open the motor relay on emergency stop
`swr~motor.estop-relay-c2v6~1`

The software opens the motor relay within 10 ms after the emergency
stop input becomes active.

Status: approved

Covers:
- sys~motor.emergency-stop-h3x9~1

Needs: dsn
