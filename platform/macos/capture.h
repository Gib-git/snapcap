// Still screenshots through ScreenCaptureKit. SnapCap's own windows are excluded.
#pragma once

#include <functional>
#include <vector>

#include "snapcap/error.h"
#include "snapcap/geometry.h"

namespace snapcap::mac {

// Captures each display at full pixel resolution, in parallel. `done` runs on the main thread.
void capture_displays(std::vector<MonitorInfo> which, std::function<void(Result<std::vector<Shot>>)> done);

}  // namespace snapcap::mac
