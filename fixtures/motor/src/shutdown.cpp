#include "shutdown.hpp"

// [impl->dsn~motor.shutdown-signal-w9e4~1]
void Shutdown::on_overtemp(bool overtemp) {
    overtemp_cycles_ = overtemp ? overtemp_cycles_ + 1 : 0;
    if (overtemp_cycles_ >= 2) {
        pwm_enabled_ = false;
    }
}
