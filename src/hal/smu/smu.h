
#include <memory>

enum class CHANNEL
{
    A,
    B
};

class SMU
{
private:
    /* data */
public:
    virtual void intitalize(std::unique_ptr<CHANNEL> channel) = 0;
    virtual double i_meas() = 0;
    virtual double v_meas() = 0;

    virtual ~SMU() = default;
};
