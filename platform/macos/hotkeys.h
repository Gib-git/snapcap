// System-wide keyboard shortcuts through Carbon's RegisterEventHotKey, which needs no
// accessibility permission and works while SnapCap is in the background.
#pragma once

#include <cstdint>
#include <functional>
#include <map>

#include "snapcap/actions.h"
#include "snapcap/error.h"
#include "snapcap/hotkeys.h"

namespace snapcap::mac {

class GlobalHotkeys {
 public:
  using Handler = std::function<void(Action)>;  // called on the main thread

  explicit GlobalHotkeys(Handler handler);
  ~GlobalHotkeys();
  GlobalHotkeys(const GlobalHotkeys&) = delete;
  GlobalHotkeys& operator=(const GlobalHotkeys&) = delete;

  Result<void> add(Action action, const Shortcut& shortcut);
  void clear();

 private:
  struct Impl;
  Impl* impl_;
};

}  // namespace snapcap::mac
