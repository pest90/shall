# L3D – Design Decisions

## Temperature monitoring in the 10 ms task
`dsn~motor.temp-monitor-r5t1~1`

The task `TempMonitor` reads the winding sensor every 10 ms and raises
the overtemperature flag above 85 °C.

Status: approved

Covers:
- swr~motor.overtemp-shutdown-k4f2~1

Needs: impl, utest

## Shutdown signal
`dsn~motor.shutdown-signal-w9e4~1`

`Shutdown::on_overtemp` switches off the PWM output in the same task
cycle in which the overtemperature flag is raised.

Status: approved

Covers:
- swr~motor.overtemp-shutdown-k4f2~2

Needs: impl, utest

## Relay driver
`dsn~motor.relay-driver-b6j3~1`

`RelayDriver` opens the relay directly from the emergency stop
interrupt.

Status: approved

Covers:
- swr~motor.estop-relay-c2v6~1

Needs: impl, utest, vrev
