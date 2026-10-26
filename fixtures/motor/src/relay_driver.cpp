#include "relay_driver.hpp"

// [impl->dsn~motor.relay-driver-b6j3~1]
void RelayDriver::on_estop() {
    closed_ = false;
}
