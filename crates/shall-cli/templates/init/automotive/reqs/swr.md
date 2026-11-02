# L3 – Software requirements

## Shut down the motor within 100 ms
`@@swr~overtemp-shutdown-time@@`

The software switches off the motor within 100 ms after the winding
temperature exceeds 85 °C.

Covers:
- @@sys~overtemp-shutdown@@

## Send the temperature message
`@@swr~temperature-message@@`

The software sends the winding temperature on the vehicle bus once a
second.

Covers:
- @@hlr~report-motor-temperature|sys@@
