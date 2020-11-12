#pragma once
#include <string>

namespace rust
{
    namespace behavior
    {
        template <typename Try, typename Fail>
        void trycatch(Try &&func, Fail &&fail) noexcept
        try
        {
            func();
        }
        catch (const ::std::exception &e)
        {
            fail(e.what());
        }
        catch (const Pylon::GenericException &e)
        {
            std::stringstream ss;
            // ss << "Pylon::GenericException: " << e.GetDescription() << " " << e.GetSourceFileName() << ":" << e.GetSourceLine();
            ss << "Pylon::GenericException: " << e.what();
            auto msg = ss.str();
            fail(msg.c_str());
        }
    } // namespace behavior
} // namespace rust
