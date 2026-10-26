#include <gtest/gtest.h>

#include "temp_monitor.hpp"

// [utest->dsn~motor.temp-monitor-r5t1~1]
TEST(TempMonitor, RaisesFlagAbove85C) {
    TempMonitor monitor;
    monitor.run(85.5f);
    EXPECT_TRUE(monitor.overtemp());
}
