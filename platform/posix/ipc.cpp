#include "ipc.h"

#include <cerrno>
#include <cstdlib>
#include <functional>
#include <cstring>
#include <format>
#include <string>
#include <system_error>

#include <poll.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include "snapcap/log.h"
#include "snapcap/paths.h"

namespace snapcap::ipc {
namespace fs = std::filesystem;
namespace {

constexpr int kTimeoutMs = 1000;

bool make_address(const fs::path& path, sockaddr_un& addr) {
  std::string s = path.string();
  if (s.size() >= sizeof(addr.sun_path)) return false;
  std::memset(&addr, 0, sizeof(addr));
  addr.sun_family = AF_UNIX;
  std::memcpy(addr.sun_path, s.c_str(), s.size() + 1);
  return true;
}

sockaddr* as_sockaddr(sockaddr_un& addr) {
  return reinterpret_cast<sockaddr*>(&addr);
}

bool wait_for(int fd, short events) {
  pollfd p{fd, events, 0};
  return ::poll(&p, 1, kTimeoutMs) == 1 && (p.revents & events) != 0;
}

}  // namespace

// Socket paths are limited to ~104 bytes, so the socket lives in a short per-user
// directory rather than next to the settings.
fs::path runtime_dir() {
#if defined(__APPLE__)
  char buf[1024];
  size_t n = confstr(_CS_DARWIN_USER_TEMP_DIR, buf, sizeof(buf));
  if (n > 0 && n <= sizeof(buf)) return fs::path(buf);
#endif
  if (const char* xdg = std::getenv("XDG_RUNTIME_DIR"); xdg != nullptr && *xdg != '\0') return fs::path(xdg);
  return fs::path("/tmp");
}

fs::path socket_path() {
  // One instance per settings folder, so test instances never talk to the real one.
  std::string config = paths::config_dir().value_or(fs::path()).string();
  auto tag = static_cast<uint32_t>(std::hash<std::string>{}(config));
  return runtime_dir() / std::format("snapcap-{}-{:08x}.sock", getuid(), tag);
}

bool send_to_running(Action action) {
  sockaddr_un addr{};
  if (!make_address(socket_path(), addr)) return false;
  int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
  if (fd < 0) return false;
  bool ok = ::connect(fd, as_sockaddr(addr), sizeof(addr)) == 0;
  if (ok) {
    std::string line = std::format("{}\n", cli_name(action));
    ok = ::write(fd, line.data(), line.size()) == static_cast<ssize_t>(line.size());
    // Wait for the server's acknowledgement so we know it really handled it.
    char ack = 0;
    ok = ok && wait_for(fd, POLLIN) && ::read(fd, &ack, 1) == 1 && ack == '1';
  }
  ::close(fd);
  return ok;
}

Server::~Server() { stop(); }

Result<void> Server::start(Handler handler) {
  handler_ = std::move(handler);
  path_ = socket_path();
  sockaddr_un addr{};
  if (!make_address(path_, addr)) return fail("socket path too long");
  fd_ = ::socket(AF_UNIX, SOCK_STREAM, 0);
  if (fd_ < 0) return fail(std::format("socket: {}", std::strerror(errno)));
  // A socket file left behind by a crashed instance: nobody answers on it, so replace it.
  ::unlink(path_.c_str());
  if (::bind(fd_, as_sockaddr(addr), sizeof(addr)) != 0 || ::listen(fd_, 8) != 0) {
    int err = errno;
    ::close(fd_);
    fd_ = -1;
    return fail(std::format("listen on {}: {}", path_.string(), std::strerror(err)));
  }
  thread_ = std::thread([this] { run(); });
  return {};
}

void Server::run() {
  while (!stopping_) {
    pollfd p{fd_, POLLIN, 0};
    if (::poll(&p, 1, 250) <= 0) continue;
    int conn = ::accept(fd_, nullptr, nullptr);
    if (conn < 0) continue;
    std::string line;
    char buf[64];
    while (line.find('\n') == std::string::npos && line.size() < 256 && wait_for(conn, POLLIN)) {
      ssize_t n = ::read(conn, buf, sizeof(buf));
      if (n <= 0) break;
      line.append(buf, static_cast<size_t>(n));
    }
    if (auto nl = line.find('\n'); nl != std::string::npos) line.resize(nl);
    auto action = from_cli_name(line);
    char ack = action ? '1' : '0';
    (void)::write(conn, &ack, 1);
    ::close(conn);
    if (action) {
      handler_(*action);
    } else {
      log::warn("ignoring unknown IPC message");
    }
  }
}

void Server::stop() {
  stopping_ = true;
  if (thread_.joinable()) thread_.join();
  if (fd_ >= 0) {
    ::close(fd_);
    fd_ = -1;
    ::unlink(path_.c_str());
  }
}

}  // namespace snapcap::ipc
