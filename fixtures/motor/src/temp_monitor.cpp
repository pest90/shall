#include "temp_monitor.hpp"

// [impl->dsn~motor.temp-monitor-r5t1~1]
void TempMonitor::run(float winding_temp_c) {
    overtemp_ = winding_temp_c > 85.0f;
}
