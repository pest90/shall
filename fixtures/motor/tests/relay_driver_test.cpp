#include <gtest/gtest.h>

#include "relay_driver.hpp"

class RelayDriverTest : public ::testing::Test {
protected:
    RelayDriver relay;
};

// [utest->dsn~motor.relay-driver-b6j3~1]
TEST_F(RelayDriverTest, OpensOnEstop) {
    relay.on_estop();
    EXPECT_FALSE(relay.closed());
}
