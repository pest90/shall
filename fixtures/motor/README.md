# Fixture `motor`

A small motor controller used as the reference project for the
integration tests of `shall` (work package 0.2 in `docs/konzept.md`).

- Three levels: `sys` (system), `swr` (software), `dsn` (design, leaf).
- Eight requirements in `reqs/`, C++ code in `src/`, gtest tests in
  `tests/` and the gtest result in `test-results/gtest.xml`.
- The C++ code is not built; the JUnit XML is checked in as if the tests
  had run.

## Deliberate defects

`expected-defects.txt` is the golden expectation: `shall check` on this
directory must report exactly these defects, sorted by path and line, in
the format `path:line: message` with paths relative to this directory.
A defect of an item defined in a requirement file points at the line
of its ID or of the offending link; a defect of an item generated from a
code or test tag points at the line of the tag.

| Defect | Location | Why |
| --- | --- | --- |
| Outdated revision | `reqs/design.md:12` | `dsn~motor.temp-monitor-r5t1~1` covers `swr~motor.overtemp-shutdown-k4f2~1`, but that item is at revision 2 (reaction time changed from 200 ms to 100 ms). |
| Uncovered requirement | `reqs/software.md:20` | `swr~motor.overtemp-warning-p8n5~1` needs `dsn`, but no design item covers it. |
| Orphaned code tag | `src/fault_log.cpp:3` | The tag points to `dsn~motor.fault-log-z2d8~1`, which does not exist. |
| Failing test | `tests/shutdown_test.cpp:5` | `Shutdown::on_overtemp` debounces over two cycles, but the design demands a shutdown in the same cycle, so `Shutdown.DisablesPwmOnOvertemp` fails. |

Only these root causes are defects. Their consequences show up as
coverage status, not as further defects: for example
`sys~motor.thermal-protection-q7m2~1` is covered but not deep covered,
because `swr~motor.overtemp-warning-p8n5~1` below it is uncovered.

`sys~motor.emergency-stop-h3x9~1` and its chain down to the relay driver
are complete and green.
