#pragma once

class Shutdown {
public:
    void on_overtemp(bool overtemp);
    bool pwm_enabled() const { return pwm_enabled_; }

private:
    bool pwm_enabled_ = true;
    int overtemp_cycles_ = 0;
};
