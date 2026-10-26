#pragma once

class TempMonitor {
public:
    void run(float winding_temp_c);
    bool overtemp() const { return overtemp_; }

private:
    bool overtemp_ = false;
};
