// Splits row-wise pixel work across cores.
#pragma once

#include <algorithm>
#include <cstddef>
#include <thread>
#include <vector>

namespace snapcap {

// Calls `fn(begin, end)` for disjoint chunks covering [0, count), on up to 8 threads.
// Small jobs run inline. Returns once every chunk is done.
template <class Fn>
void parallel_for(size_t count, size_t min_per_thread, Fn&& fn) {
  size_t hw = std::max<size_t>(1, std::thread::hardware_concurrency());
  size_t threads = std::clamp<size_t>(count / std::max<size_t>(1, min_per_thread), 1, std::min<size_t>(hw, 8));
  if (threads <= 1) {
    fn(size_t{0}, count);
    return;
  }
  size_t per = (count + threads - 1) / threads;
  std::vector<std::jthread> pool;
  pool.reserve(threads);
  for (size_t begin = 0; begin < count; begin += per) {
    size_t end = std::min(count, begin + per);
    pool.emplace_back([&fn, begin, end] { fn(begin, end); });
  }
}

}  // namespace snapcap
