#include "ki2636b.h"

void ki2636b::intitalize(std::unique_ptr<CHANNEL> channel)
{
    switch (*channel)
    {
    case CHANNEL::A:
        /* code */
        break;
    case CHANNEL::B:
        /* code */
        break;
    default:
        break;
    }
}

double ki2636b::i_meas()
{
    return 0.0;
}

double ki2636b::v_meas()
{
    return 0.0;
}
