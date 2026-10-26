#include <gtest/gtest.h>

#include "shutdown.hpp"

// [utest->dsn~motor.shutdown-signal-w9e4~1]
TEST(Shutdown, DisablesPwmOnOvertemp) {
    Shutdown shutdown;
    shutdown.on_overtemp(true);
    EXPECT_FALSE(shutdown.pwm_enabled());
}
