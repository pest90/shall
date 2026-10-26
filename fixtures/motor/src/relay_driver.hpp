#pragma once

class RelayDriver {
public:
    void on_estop();
    bool closed() const { return closed_; }

private:
    bool closed_ = true;
};
