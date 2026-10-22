// Error handling: fallible functions return Result<T>, never throw across module boundaries.
#pragma once

#include <expected>
#include <string>
#include <utility>

namespace snapcap {

struct Error {
  std::string message;
};

template <class T = void>
using Result = std::expected<T, Error>;

inline std::unexpected<Error> fail(std::string message) {
  return std::unexpected(Error{std::move(message)});
}

}  // namespace snapcap
