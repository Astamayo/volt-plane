#include "smu.h"

class ki2636b : public SMU {
public:
    void intitalize(std::unique_ptr<CHANNEL> channel) override;
    double i_meas() override;
    double v_meas() override;
};