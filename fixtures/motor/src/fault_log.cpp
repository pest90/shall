#include <cstdio>

// [impl->dsn~motor.fault-log-z2d8~1]
void log_fault(const char* message) {
    std::fprintf(stderr, "fault: %s\n", message);
}
