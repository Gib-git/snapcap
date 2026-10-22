// NeuQuant neural-net quantization, after Anthony Dekker's 1994 algorithm (as used by the
// Rust `color_quant` crate), reduced to RGB because GIF frames here are always opaque.
#include <algorithm>
#include <cmath>
#include <limits>

#include "snapcap/gif.h"

namespace snapcap {
namespace {

constexpr int kRadiusDec = 30;
constexpr int kInitAlpha = 1 << 10;
constexpr double kBeta = 1.0 / 1024.0;
constexpr double kBetaGamma = kBeta * 1024.0;
constexpr int kRadiusBiasShift = 6;
constexpr size_t kPrimes[] = {499, 491, 487, 503};

int clamp_channel(double v) {
  return std::clamp(static_cast<int>(std::lround(v)), 0, 255);
}

}  // namespace

NeuQuant::NeuQuant(std::span<const uint8_t> rgba, int sample_factor, size_t colors)
    : net_size_(std::clamp<size_t>(colors, 2, 256)) {
  network_.reserve(net_size_);
  for (size_t i = 0; i < net_size_; ++i) {
    double v = double(i) * 256.0 / double(net_size_);
    network_.push_back({v, v, v});
  }
  freq_.assign(net_size_, 1.0 / double(net_size_));
  bias_.assign(net_size_, 0.0);
  learn(rgba, std::max(1, sample_factor));
  build_index();
}

size_t NeuQuant::contest(double r, double g, double b) {
  double best_d = std::numeric_limits<double>::max();
  double best_bias_d = best_d;
  size_t best = 0, best_bias = 0;
  for (size_t i = 0; i < net_size_; ++i) {
    const Neuron& n = network_[i];
    double dist = std::abs(n.b - b) + std::abs(n.r - r);
    if (dist < best_d || dist < best_bias_d + bias_[i]) {
      dist += std::abs(n.g - g);
      if (dist < best_d) {
        best_d = dist;
        best = i;
      }
      double bias_dist = dist - bias_[i];
      if (bias_dist < best_bias_d) {
        best_bias_d = bias_dist;
        best_bias = i;
      }
    }
    freq_[i] -= kBeta * freq_[i];
    bias_[i] += kBetaGamma * freq_[i];
  }
  freq_[best] += kBeta;
  bias_[best] -= kBetaGamma;
  return best_bias;
}

void NeuQuant::alter_neighbours(double alpha, int radius, size_t i, double r, double g, double b) {
  const auto net = static_cast<int>(net_size_);
  const int center = static_cast<int>(i);
  const int lo = std::max(center - radius, 0);
  const int hi = std::min(center + radius, net);
  int j = center + 1, k = center - 1, q = 0;
  const double rad_sq = double(radius) * radius;
  while (j < hi || k > lo) {
    double a = alpha * (rad_sq - double(q) * q) / rad_sq;
    ++q;
    auto pull = [&](Neuron& n) {
      n.r -= a * (n.r - r);
      n.g -= a * (n.g - g);
      n.b -= a * (n.b - b);
    };
    if (j < hi) pull(network_[static_cast<size_t>(j++)]);
    if (k > lo) pull(network_[static_cast<size_t>(k--)]);
  }
}

void NeuQuant::learn(std::span<const uint8_t> rgba, int sample_factor) {
  const size_t length = rgba.size() / 4;
  if (length == 0) return;
  const int init_radius = static_cast<int>(net_size_ / 8);
  int bias_radius = init_radius * (1 << kRadiusBiasShift);
  const int alpha_dec = 30 + (sample_factor - 1) / 3;
  const size_t sample_pixels = length / static_cast<size_t>(sample_factor);
  const size_t cycles = std::max<size_t>(100, net_size_ >> 1);
  const size_t delta = std::max<size_t>(1, sample_pixels / cycles);
  int alpha = kInitAlpha;
  int radius = bias_radius >> kRadiusBiasShift;
  if (radius <= 1) radius = 0;

  size_t step = kPrimes[3];
  for (size_t p : kPrimes) {
    if (length % p != 0) {
      step = p;
      break;
    }
  }

  size_t pos = 0;
  for (size_t i = 0; i < sample_pixels;) {
    const uint8_t* px = rgba.data() + pos * 4;
    double r = px[0], g = px[1], b = px[2];
    size_t j = contest(r, g, b);
    double a = double(alpha) / kInitAlpha;
    Neuron& n = network_[j];
    n.r -= a * (n.r - r);
    n.g -= a * (n.g - g);
    n.b -= a * (n.b - b);
    if (radius > 0) alter_neighbours(a, radius, j, r, g, b);

    pos = (pos + step) % length;
    ++i;
    if (i % delta == 0) {
      alpha -= alpha / alpha_dec;
      bias_radius -= bias_radius / kRadiusDec;
      radius = bias_radius >> kRadiusBiasShift;
      if (radius <= 1) radius = 0;
    }
  }
}

void NeuQuant::build_index() {
  sorted_.clear();
  for (size_t i = 0; i < net_size_; ++i) {
    const Neuron& n = network_[i];
    sorted_.push_back({clamp_channel(n.r), clamp_channel(n.g), clamp_channel(n.b), static_cast<uint8_t>(i)});
  }
  std::ranges::stable_sort(sorted_, {}, &Entry::g);
  green_index_.assign(256, 0);
  size_t pos = 0;
  for (int v = 0; v < 256; ++v) {
    while (pos + 1 < sorted_.size() && sorted_[pos].g < v) ++pos;
    green_index_[static_cast<size_t>(v)] = pos;
  }
}

uint8_t NeuQuant::index_of(uint8_t r, uint8_t g, uint8_t b) const {
  // Walk outwards from the closest green value; stop each direction once green alone is too far.
  int best_d = std::numeric_limits<int>::max();
  size_t best = 0;
  auto try_entry = [&](size_t idx) {
    const Entry& e = sorted_[idx];
    int dg = e.g - g;
    int d = dg * dg;
    if (d >= best_d) return false;
    int dr = e.r - r, db = e.b - b;
    d += dr * dr + db * db;
    if (d < best_d) {
      best_d = d;
      best = idx;
    }
    return true;
  };
  size_t start = green_index_[g];
  bool up = true, down = start > 0;
  size_t i = start, j = start;  // j is one past the next lower index
  while (up || down) {
    if (up) up = try_entry(i) && ++i < sorted_.size();
    if (down) down = try_entry(j - 1) && --j > 0;
  }
  return sorted_[best].original;
}

std::vector<uint8_t> NeuQuant::palette_rgb() const {
  std::vector<uint8_t> out(net_size_ * 3);
  for (const Entry& e : sorted_) {
    out[size_t{e.original} * 3 + 0] = static_cast<uint8_t>(e.r);
    out[size_t{e.original} * 3 + 1] = static_cast<uint8_t>(e.g);
    out[size_t{e.original} * 3 + 2] = static_cast<uint8_t>(e.b);
  }
  return out;
}

}  // namespace snapcap
