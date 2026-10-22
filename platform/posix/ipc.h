// Single-instance support over a Unix domain socket. A second launch (e.g. `snapcap
// --shot region` bound to a desktop shortcut) forwards its action to the running
// instance and exits. The protocol is one line per connection: the action's CLI name.
#pragma once

#include <atomic>
#include <filesystem>
#include <functional>
#include <thread>

#include "snapcap/actions.h"
#include "snapcap/error.h"

namespace snapcap::ipc {

std::filesystem::path socket_path();

// Returns true if a running instance accepted the action.
bool send_to_running(Action action);

class Server {
 public:
  using Handler = std::function<void(Action)>;  // called on the listener thread

  Server() = default;
  ~Server();
  Server(const Server&) = delete;
  Server& operator=(const Server&) = delete;

  Result<void> start(Handler handler);
  void stop();

 private:
  void run();

  Handler handler_;
  int fd_ = -1;
  std::atomic<bool> stopping_{false};
  std::thread thread_;
  std::filesystem::path path_;
};

}  // namespace snapcap::ipc
